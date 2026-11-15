// AI-assisted
//! Axum router and HTTP handlers for Public Demo Mode, Argon2id Researcher Auth,
//! protected catalog/weight management, 512 MiB binary `multipart/form-data` 3D/DEM terrain
//! upload & topological analysis, and static WASM/asset serving.

use std::collections::BTreeMap;
use std::fs;

use analog_core::scoring::{compute_weighted_score, normalize_weights, recalculate_catalog};
use analog_core::stl::MAX_TRIANGLES;
use analog_core::terrain_analysis::{
    UploadSiteRequest, UploadSiteResponse, analyze_and_build_site_bundle,
    decode_multipart_upload_request,
};
use analog_core::{Catalog, RecalculateRequest, RecalculateResponse, SiteMetadata};
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing::{info, warn};

use crate::auth::{
    build_logout_cookie_header, build_session_cookie_header_from_token, create_session_token,
    extract_authenticated_researcher, extract_client_key, verify_argon2_password,
};
use crate::state::{AppState, default_synthetic_catalog};

/// Optional request body for `POST /api/sites/reset`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResetCatalogRequest {
    /// When `true`, restores the reference synthetic `test_site`; when `false` (default),
    /// clears all preloaded sites from `catalog.json` so only newly uploaded maps appear.
    #[serde(default)]
    pub restore_demo: bool,
}

/// Maximum request body size for `/api/sites/upload` (512 MiB to accommodate full batches of
/// 50–100+ MiB real `.stl`, `.tif`/`.tiff`, and `.png` files via binary `multipart/form-data`).
pub const MAX_UPLOAD_BODY_BYTES: usize = 512 * 1024 * 1024;

/// Standard JSON error payload (always in English per `AGENTS.md` §4.3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiErrorResponse {
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after_seconds: Option<u64>,
}

/// Login request body for `POST /api/login`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

/// Authentication / session state response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStatusResponse {
    pub authenticated: bool,
    pub mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_token: Option<String>,
    pub message: String,
    pub generated_by: String,
}

/// Public Demo Mode payload returned by `GET /api/demo` for passwordless judge access.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DemoModeResponse {
    pub mode: String,
    pub password_required: bool,
    pub message: String,
    pub site_count: usize,
    pub default_weights: BTreeMap<String, f64>,
    pub catalog: Catalog,
    pub generated_by: String,
}

fn json_error(status: StatusCode, msg: impl Into<String>) -> Response {
    let body = ApiErrorResponse {
        error: msg.into(),
        retry_after_seconds: None,
    };
    (status, Json(body)).into_response()
}

fn require_researcher(headers: &HeaderMap, state: &AppState) -> Result<String, Response> {
    extract_authenticated_researcher(
        headers,
        &state.config.session_secret,
        &state.config.researcher_username,
    )
    .ok_or_else(|| {
        json_error(
            StatusCode::UNAUTHORIZED,
            "Authentication required. Please log in with researcher credentials at POST /api/login.",
        )
    })
}

/// Builds the full Axum [`Router`] with API endpoints and static file services.
pub fn build_router(state: AppState) -> Router {
    let data_service_root = ServeDir::new(&state.config.data_dir);
    let data_service_gh_pages = ServeDir::new(&state.config.data_dir);
    let static_service_root =
        ServeDir::new(&state.config.static_dir).append_index_html_on_directories(true);
    let static_service_gh_pages =
        ServeDir::new(&state.config.static_dir).append_index_html_on_directories(true);

    let api_router = Router::new()
        .route("/health", get(health_handler))
        .route("/demo", get(demo_mode_handler))
        .route("/catalog", get(get_catalog_handler))
        .route("/recalculate", post(recalculate_handler))
        .route("/me", get(session_status_handler))
        .route("/login", post(login_handler))
        .route("/logout", post(logout_handler))
        .route(
            "/weights",
            put(update_weights_handler).post(update_weights_handler),
        )
        .route("/sites", post(upsert_site_body_handler))
        .route("/sites/upload", post(upload_and_analyze_site_handler))
        .route("/sites/reset", post(reset_catalog_handler))
        .route("/sites/{id}", put(upsert_site_path_handler))
        .layer(DefaultBodyLimit::max(MAX_UPLOAD_BODY_BYTES));

    Router::new()
        .nest("/api", api_router.clone())
        .nest("/Space-App-X86/api", api_router)
        .nest_service("/data/processed", data_service_root)
        .nest_service("/Space-App-X86/data/processed", data_service_gh_pages)
        .nest_service("/Space-App-X86", static_service_gh_pages)
        .fallback_service(static_service_root)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// `GET /api/health` — Simple liveness check.
async fn health_handler() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "ok",
        "service": "analog_server",
        "generated_by": "analog_server"
    }))
}

/// `GET /api/demo` — Passwordless entry endpoint for Space Apps judges.
async fn demo_mode_handler(State(state): State<AppState>) -> impl IntoResponse {
    let catalog = state
        .catalog
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();

    Json(DemoModeResponse {
        mode: "demo".to_string(),
        password_required: false,
        message: "Public Demo Mode is active. Judges can inspect 3D terrains, toggle the UV checkerboard test, and run custom weight recalculations without a password.".to_string(),
        site_count: catalog.sites.len(),
        default_weights: catalog.default_weights.clone(),
        catalog,
        generated_by: "analog_server".to_string(),
    })
}

/// `GET /api/catalog` — Public read access to the site catalog (`data/processed/catalog.json`).
async fn get_catalog_handler(State(state): State<AppState>) -> impl IntoResponse {
    let catalog = state
        .catalog
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    Json(catalog)
}

/// `POST /api/recalculate` — Public & researcher endpoint to compute weighted site rankings
/// over pre-normalized `[0.0, 1.0]` feature scores using `analog_core::scoring`.
async fn recalculate_handler(
    State(state): State<AppState>,
    Json(req): Json<RecalculateRequest>,
) -> Result<Json<RecalculateResponse>, Response> {
    if req.weights.is_empty() {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "Weights map must not be empty.",
        ));
    }

    let catalog = state
        .catalog
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();

    let response = recalculate_catalog(&catalog, &req.weights);
    Ok(Json(response))
}

/// `GET /api/me` — Returns current session status (`researcher` if authenticated, `demo` otherwise).
async fn session_status_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Some(username) = extract_authenticated_researcher(
        &headers,
        &state.config.session_secret,
        &state.config.researcher_username,
    ) {
        let token = create_session_token(
            &username,
            &state.config.session_secret,
            state.config.session_ttl_seconds,
        );
        Json(SessionStatusResponse {
            authenticated: true,
            mode: "researcher".to_string(),
            username: Some(username),
            session_token: Some(token),
            message: "Authenticated researcher session active.".to_string(),
            generated_by: "analog_server".to_string(),
        })
    } else {
        Json(SessionStatusResponse {
            authenticated: false,
            mode: "demo".to_string(),
            username: None,
            session_token: None,
            message: "Public Demo Mode active (no login required for viewing and recalculation)."
                .to_string(),
            generated_by: "analog_server".to_string(),
        })
    }
}

/// `POST /api/login` — Authenticates a researcher using Argon2id, enforces per-IP rate limiting,
/// and issues both a signed `HttpOnly; SameSite=Lax` session cookie and a `session_token` for WASM/LAN clients.
async fn login_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<LoginRequest>,
) -> Response {
    let client_key = extract_client_key(&headers);
    if let Err(retry_after) = state.rate_limiter.check_and_record(&client_key) {
        let body = ApiErrorResponse {
            error: format!(
                "Too many login attempts from this address. Please try again in {retry_after} seconds."
            ),
            retry_after_seconds: Some(retry_after),
        };
        let mut resp = (StatusCode::TOO_MANY_REQUESTS, Json(body)).into_response();
        if let Ok(val) = HeaderValue::from_str(&retry_after.to_string()) {
            resp.headers_mut().insert(header::RETRY_AFTER, val);
        }
        return resp;
    }

    let Some(expected_hash) = state.config.researcher_password_hash.as_deref() else {
        warn!("POST /api/login rejected: RESEARCHER_PASSWORD_HASH is not configured");
        return json_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Researcher login is disabled because RESEARCHER_PASSWORD_HASH is not set in the server environment (.env). Use Public Demo Mode instead.",
        );
    };

    let submitted_user = req.username.trim();
    let expected_user = state.config.researcher_username.trim();
    let username_matches = submitted_user == expected_user;
    let password_matches = match verify_argon2_password(&req.password, expected_hash) {
        Ok(valid) => valid,
        Err(err) => {
            warn!("Argon2 configuration error during POST /api/login: {err}");
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, err);
        }
    };

    if !username_matches || !password_matches {
        warn!(
            "Failed researcher login attempt for user '{}' (expected '{}', username_match={}, password_match={})",
            submitted_user, expected_user, username_matches, password_matches
        );
        return json_error(
            StatusCode::UNAUTHORIZED,
            format!(
                "Invalid username or password (expected researcher username: '{expected_user}')."
            ),
        );
    }

    // Clear rate-limit bucket on successful authentication
    state.rate_limiter.clear_for_client(&client_key);

    let token = create_session_token(
        expected_user,
        &state.config.session_secret,
        state.config.session_ttl_seconds,
    );
    let cookie_val = build_session_cookie_header_from_token(
        &token,
        state.config.session_ttl_seconds,
        state.config.cookie_secure,
    );

    info!("Researcher '{}' authenticated via Argon2id", expected_user);

    let body = SessionStatusResponse {
        authenticated: true,
        mode: "researcher".to_string(),
        username: Some(expected_user.to_string()),
        session_token: Some(token),
        message: format!("Researcher '{expected_user}' logged in (Argon2id verified)."),
        generated_by: "analog_server".to_string(),
    };

    let mut resp = (StatusCode::OK, Json(body)).into_response();
    if let Ok(header_val) = HeaderValue::from_str(&cookie_val) {
        resp.headers_mut().insert(header::SET_COOKIE, header_val);
    }
    resp
}

/// `POST /api/logout` — Clears the researcher session cookie and returns to Public Demo Mode.
async fn logout_handler(State(state): State<AppState>) -> Response {
    let cookie_val = build_logout_cookie_header(state.config.cookie_secure);
    let body = SessionStatusResponse {
        authenticated: false,
        mode: "demo".to_string(),
        username: None,
        session_token: None,
        message: "Researcher session ended. Returned to Public Demo Mode.".to_string(),
        generated_by: "analog_server".to_string(),
    };

    let mut resp = (StatusCode::OK, Json(body)).into_response();
    if let Ok(header_val) = HeaderValue::from_str(&cookie_val) {
        resp.headers_mut().insert(header::SET_COOKIE, header_val);
    }
    resp
}

/// `PUT /api/weights` — Authenticated researcher endpoint to update and persist canonical
/// feature weights and recalculate canonical scores across the catalog.
async fn update_weights_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<RecalculateRequest>,
) -> Result<Json<Catalog>, Response> {
    let _researcher = require_researcher(&headers, &state)?;

    let normalized = normalize_weights(&req.weights);
    let sum: f64 = normalized.values().sum();
    if normalized.is_empty() || sum <= 0.0 {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "At least one positive finite weight is required to update canonical weights.",
        ));
    }

    let updated_catalog = {
        let mut catalog = state
            .catalog
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        catalog.default_weights = normalized.clone();
        for site in &mut catalog.sites {
            site.canonical_score = compute_weighted_score(&site.feature_scores, &normalized);
        }
        catalog.sites.sort_by(|a, b| {
            b.canonical_score
                .partial_cmp(&a.canonical_score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.id.cmp(&b.id))
        });
        catalog.generated_by = Some("analog_server (authenticated researcher)".to_string());
        catalog.clone()
    };

    let _ = state.persist_weights(&normalized);
    let _ = state.persist_catalog(&updated_catalog);

    Ok(Json(updated_catalog))
}

/// `POST /api/sites/upload` — Authenticated researcher endpoint accepting binary `multipart/form-data`
/// (up to 512 MiB, zero Base64 overhead) or `application/json` to upload `.stl`, `.tif`/`.tiff`,
/// and/or `.png` files, automatically simplify meshes exceeding `50,000` triangles, execute
/// morphometric + 0D Persistent Homology TDA, persist the optimized assets to `data/processed/<site_id>/`,
/// and register the site in `catalog.json`.
async fn upload_and_analyze_site_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<UploadSiteResponse>, Response> {
    let researcher = require_researcher(&headers, &state)?;

    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let req: UploadSiteRequest = if content_type
        .to_ascii_lowercase()
        .contains("multipart/form-data")
    {
        decode_multipart_upload_request(&content_type, &body)
            .map_err(|e| json_error(StatusCode::BAD_REQUEST, e))?
    } else {
        serde_json::from_slice::<UploadSiteRequest>(&body).map_err(|e| {
            json_error(
                StatusCode::BAD_REQUEST,
                format!("Invalid upload payload (expected multipart/form-data or JSON): {e}"),
            )
        })?
    };

    let current_catalog = state
        .catalog
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();

    let bundle = tokio::task::spawn_blocking(move || {
        analyze_and_build_site_bundle(&req, &current_catalog)
    })
    .await
    .map_err(|e| {
        json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Terrain analysis worker panicked: {e}"),
        )
    })?
    .map_err(|e| json_error(StatusCode::BAD_REQUEST, e))?;

    let site_id = bundle.report.site_id.clone();
    let site_metadata = bundle.report.generated_site_metadata.clone();

    // Persist optimized terrain.stl (<= 50,000 triangles), dem.tif, albedo.png, and metadata.json
    // under data/processed/<site_id>/
    let site_dir = state.config.data_dir.join(&site_id);
    if let Err(e) = fs::create_dir_all(&site_dir) {
        return Err(json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!(
                "Failed to create site directory {}: {e}",
                site_dir.display()
            ),
        ));
    }

    let stl_path = site_dir.join("terrain.stl");
    if let Err(e) = fs::write(&stl_path, &bundle.stl_bytes) {
        return Err(json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to write {}: {e}", stl_path.display()),
        ));
    }

    let tiff_path = site_dir.join("dem.tif");
    if let Err(e) = fs::write(&tiff_path, &bundle.tiff_bytes) {
        return Err(json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to write {}: {e}", tiff_path.display()),
        ));
    }

    let png_path = site_dir.join("albedo.png");
    if let Err(e) = fs::write(&png_path, &bundle.png_bytes) {
        return Err(json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to write {}: {e}", png_path.display()),
        ));
    }

    let metadata_path = site_dir.join("metadata.json");
    if let Ok(meta_json) = serde_json::to_string_pretty(&site_metadata) {
        let _ = fs::write(&metadata_path, meta_json);
    }

    info!(
        "Researcher '{}' uploaded & analyzed site '{}' (triangles: {} -> {}, analog_score={:.4}, closest='{}')",
        researcher,
        site_id,
        bundle.report.original_triangle_count,
        bundle.report.simplified_triangle_count,
        bundle.report.proposal.analog_score,
        bundle.report.proposal.closest_site_id
    );

    let updated_catalog = {
        let mut catalog = state
            .catalog
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if let Some(existing) = catalog
            .sites
            .iter_mut()
            .find(|s| s.id == site_metadata.id)
        {
            *existing = site_metadata;
        } else {
            catalog.sites.push(site_metadata);
        }

        catalog.sites.sort_by(|a, b| {
            b.canonical_score
                .partial_cmp(&a.canonical_score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.id.cmp(&b.id))
        });
        catalog.generated_by = Some(format!(
            "analog_server (uploaded & analyzed by {researcher})"
        ));
        catalog.clone()
    };

    let _ = state.persist_catalog(&updated_catalog);

    Ok(Json(UploadSiteResponse {
        report: bundle.report,
        updated_catalog,
        generated_by: "AI-assisted (analog_server::upload_and_analyze_site)".to_string(),
    }))
}

/// `POST /api/sites` — Authenticated researcher endpoint to insert or update a site in the catalog.
async fn upsert_site_body_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(site): Json<SiteMetadata>,
) -> Result<Json<SiteMetadata>, Response> {
    upsert_site_inner(state, headers, None, site)
}

/// `PUT /api/sites/{id}` — Authenticated researcher endpoint to update a site by ID.
async fn upsert_site_path_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(site): Json<SiteMetadata>,
) -> Result<Json<SiteMetadata>, Response> {
    upsert_site_inner(state, headers, Some(id), site)
}

fn upsert_site_inner(
    state: AppState,
    headers: HeaderMap,
    path_id: Option<String>,
    mut site: SiteMetadata,
) -> Result<Json<SiteMetadata>, Response> {
    let _researcher = require_researcher(&headers, &state)?;

    if let Some(id) = path_id {
        site.id = id;
    }
    if site.id.trim().is_empty() {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "Site ID must not be empty.",
        ));
    }
    if !site.dem_extent.is_valid() {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "Invalid DemExtent: min_x < max_x and min_y < max_y must be finite.",
        ));
    }
    if site.triangle_count > MAX_TRIANGLES {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            format!(
                "Site triangle_count {} exceeds maximum budget of {} triangles.",
                site.triangle_count, MAX_TRIANGLES
            ),
        ));
    }

    let updated_site = {
        let mut catalog = state
            .catalog
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        site.canonical_score =
            compute_weighted_score(&site.feature_scores, &catalog.default_weights);
        site.generated_by = Some("analog_server (authenticated researcher)".to_string());

        if let Some(existing) = catalog.sites.iter_mut().find(|s| s.id == site.id) {
            *existing = site.clone();
        } else {
            catalog.sites.push(site.clone());
        }

        catalog.sites.sort_by(|a, b| {
            b.canonical_score
                .partial_cmp(&a.canonical_score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.id.cmp(&b.id))
        });

        let snapshot = catalog.clone();
        let _ = state.persist_catalog(&snapshot);
        site
    };

    Ok(Json(updated_site))
}

/// `POST /api/sites/reset` — Authenticated researcher endpoint to clear preloaded catalog maps
/// (or restore the reference synthetic `test_site` when `restore_demo: true` is passed) and
/// persist the clean `data/processed/catalog.json`.
async fn reset_catalog_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Option<Json<ResetCatalogRequest>>,
) -> Result<Json<Catalog>, Response> {
    let researcher = require_researcher(&headers, &state)?;
    let restore_demo = payload.map(|Json(p)| p.restore_demo).unwrap_or(false);

    let updated_catalog = {
        let mut catalog = state
            .catalog
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if restore_demo {
            let demo_catalog = default_synthetic_catalog();
            catalog.sites = demo_catalog.sites;
            let default_weights = catalog.default_weights.clone();
            for site in &mut catalog.sites {
                site.canonical_score =
                    compute_weighted_score(&site.feature_scores, &default_weights);
            }
            catalog.generated_by =
                Some("analog_server (reset_catalog: restored reference test_site)".to_string());
        } else {
            catalog.sites.clear();
            catalog.generated_by =
                Some("analog_server (reset_catalog: cleared preloaded maps)".to_string());
        }

        let snapshot = catalog.clone();
        if let Err(err) = state.persist_catalog(&snapshot) {
            warn!("Failed to persist reset catalog.json: {err}");
        }
        snapshot
    };

    info!(
        "Researcher '{}' reset catalog (restore_demo = {}, remaining sites = {})",
        researcher,
        restore_demo,
        updated_catalog.sites.len()
    );

    Ok(Json(updated_catalog))
}
