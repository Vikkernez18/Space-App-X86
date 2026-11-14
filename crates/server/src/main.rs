// AI-assisted
//! `analog_server` — Full-Mode Axum web server for Space-App-X86.
//!
//! Features:
//! - Serves compiled Trunk/Bevy WASM static files (`dist/`) and processed site assets (`data/processed/`).
//! - Public Demo Mode (`GET /api/demo`, `GET /api/catalog`, `POST /api/recalculate`) with zero login required for judges.
//! - Researcher authentication (`POST /api/login`, `POST /api/logout`, `GET /api/me`) using Argon2id,
//!   signed `HttpOnly; SameSite=Lax` session cookies, and sliding-window login rate limiting.
//! - Protected researcher endpoints (`PUT /api/weights`, `POST /api/sites`, `PUT /api/sites/{id}`)
//!   to update canonical scoring weights and site metadata.

pub mod auth;
pub mod config;
pub mod routes;
pub mod state;

use std::env;
use std::time::{SystemTime, UNIX_EPOCH};

use auth::hash_argon2_password;
use config::ServerConfig;
use routes::build_router;
use state::AppState;
use tokio::net::TcpListener;
use tracing::{info, warn};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Optional CLI helper: `cargo run -p analog_server -- --hash-password "<password>"`
    let args: Vec<String> = env::args().collect();
    if let Some(pos) = args.iter().position(|a| a == "--hash-password") {
        let Some(password) = args.get(pos + 1) else {
            eprintln!("Usage: analog_server --hash-password \"<password>\"");
            std::process::exit(1);
        };
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .to_le_bytes();
        let hash = hash_argon2_password(password, &seed)?;
        println!("{hash}");
        return Ok(());
    }

    let _ = dotenvy::dotenv();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "analog_server=info,tower_http=info".into()),
        )
        .init();

    let config = ServerConfig::from_env();
    if config.researcher_password_hash.is_none() {
        warn!(
            "RESEARCHER_PASSWORD_HASH is not set. Public Demo Mode is active, but researcher login (POST /api/login) will remain disabled until configured."
        );
    }

    let bind_addr = config.bind_addr.clone();
    let data_dir = config.data_dir.display().to_string();
    let static_dir = config.static_dir.display().to_string();

    let state = AppState::new(config);
    let app = build_router(state);

    let listener = TcpListener::bind(&bind_addr).await?;
    info!("Space-App-X86 server listening on http://{bind_addr}");
    info!("Serving processed site assets from: {data_dir}");
    info!("Serving compiled WASM viewer from : {static_dir}");
    info!("Public Demo Mode endpoint         : http://{bind_addr}/api/demo");

    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::time::Duration;

    use analog_core::{
        Body, Catalog, DemExtent, RecalculateResponse, SiteMetadata,
    };
    use axum::body::{Body as AxumBody, to_bytes};
    use axum::http::{Request, StatusCode, header};
    use tower::ServiceExt;

    use crate::auth::hash_argon2_password;
    use crate::config::ServerConfig;
    use crate::routes::{DemoModeResponse, SessionStatusResponse, build_router};
    use crate::state::AppState;

    fn sample_test_state(max_login_attempts: usize) -> (AppState, String) {
        let test_password = format!("test-only-ephemeral-{}", std::process::id());
        let phc_hash = hash_argon2_password(&test_password, b"unit-test-salt-seed")
            .expect("Argon2id hash should succeed");

        let config = ServerConfig {
            bind_addr: "127.0.0.1:0".to_string(),
            data_dir: PathBuf::from("/tmp/space_app_x86_server_test/data"),
            weights_file: PathBuf::from("/tmp/space_app_x86_server_test/weights.json"),
            static_dir: PathBuf::from("/tmp/space_app_x86_server_test/dist"),
            researcher_username: "researcher".to_string(),
            researcher_password_hash: Some(phc_hash),
            session_secret: b"unit-test-hmac-secret-key-32bytes!".to_vec(),
            session_ttl_seconds: 3600,
            cookie_secure: false,
            rate_limit_max_attempts: max_login_attempts,
            rate_limit_window: Duration::from_secs(60),
        };

        let mut default_weights = BTreeMap::new();
        default_weights.insert("slope_safety".to_string(), 0.5);
        default_weights.insert("roughness_similarity".to_string(), 0.5);

        let mut features = BTreeMap::new();
        features.insert("slope_safety".to_string(), 0.8);
        features.insert("roughness_similarity".to_string(), 0.6);

        let catalog = Catalog {
            version: "1.0.0".to_string(),
            default_weights,
            sites: vec![SiteMetadata {
                id: "test_site".to_string(),
                name: "Test Lunar Site".to_string(),
                body: Body::Moon,
                description: "Synthetic test site".to_string(),
                center_lat_lon: Some([-89.5, 0.0]),
                dem_extent: DemExtent::new(-1000.0, 1000.0, -1000.0, 1000.0),
                elevation_range_m: Some([-45.0, 120.0]),
                triangle_count: 2048,
                mesh_path: "test_site/terrain.stl".to_string(),
                mesh_format: None,
                texture_path: "test_site/checkerboard.png".to_string(),
                feature_scores: features,
                canonical_score: 0.7,
                generated_by: None,
            }],
            generated_by: None,
        };

        (AppState::with_catalog(config, catalog), test_password)
    }

    #[tokio::test]
    async fn test_public_demo_and_catalog_and_recalculate_without_login() {
        let (state, _) = sample_test_state(5);
        let app = build_router(state);

        // 1. GET /api/demo works without credentials
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/demo")
                    .body(AxumBody::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let demo: DemoModeResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(demo.mode, "demo");
        assert!(!demo.password_required);
        assert_eq!(demo.site_count, 1);

        // 2. POST /api/recalculate works publicly in Demo Mode
        let recalc_payload = serde_json::json!({
            "weights": {
                "slope_safety": 1.0,
                "roughness_similarity": 0.0
            }
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/recalculate")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(AxumBody::from(recalc_payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let recalc: RecalculateResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(recalc.results.len(), 1);
        assert!((recalc.results[0].score - 0.8).abs() < 1e-6);
    }

    #[tokio::test]
    async fn test_argon2_login_cookie_and_protected_researcher_routes() {
        let (state, password) = sample_test_state(5);
        let app = build_router(state);

        // 1. Unauthenticated PUT /api/weights must return 401 Unauthorized
        let weights_payload = serde_json::json!({
            "weights": {
                "slope_safety": 0.9,
                "roughness_similarity": 0.1
            }
        });
        let unauth_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/weights")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(AxumBody::from(weights_payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauth_resp.status(), StatusCode::UNAUTHORIZED);

        // 2. Login with valid Argon2id credentials
        let login_payload = serde_json::json!({
            "username": "researcher",
            "password": password
        });
        let login_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/login")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(AxumBody::from(login_payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(login_resp.status(), StatusCode::OK);

        let set_cookie = login_resp
            .headers()
            .get(header::SET_COOKIE)
            .expect("Set-Cookie header must be present")
            .to_str()
            .unwrap()
            .to_string();
        assert!(set_cookie.contains("HttpOnly"));
        assert!(set_cookie.contains("SameSite=Lax"));

        let cookie_pair = set_cookie.split(';').next().unwrap().to_string();

        // 3. GET /api/me with session cookie reports authenticated researcher mode
        let me_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/me")
                    .header(header::COOKIE, &cookie_pair)
                    .body(AxumBody::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(me_resp.status(), StatusCode::OK);
        let me_bytes = to_bytes(me_resp.into_body(), usize::MAX).await.unwrap();
        let me_status: SessionStatusResponse = serde_json::from_slice(&me_bytes).unwrap();
        assert!(me_status.authenticated);
        assert_eq!(me_status.mode, "researcher");

        // 4. Authenticated PUT /api/weights succeeds and updates canonical_score
        let auth_weights_resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/weights")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::COOKIE, &cookie_pair)
                    .body(AxumBody::from(weights_payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(auth_weights_resp.status(), StatusCode::OK);
        let cat_bytes = to_bytes(auth_weights_resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let updated_catalog: Catalog = serde_json::from_slice(&cat_bytes).unwrap();
        // 0.8 * 0.9 + 0.6 * 0.1 = 0.78
        assert!((updated_catalog.sites[0].canonical_score - 0.78).abs() < 1e-6);
    }

    #[tokio::test]
    async fn test_login_rate_limiting_returns_429() {
        let (state, _) = sample_test_state(2);
        let app = build_router(state);

        let bad_login = serde_json::json!({
            "username": "researcher",
            "password": "wrong"
        });

        for _ in 0..2 {
            let resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/login")
                        .header(header::CONTENT_TYPE, "application/json")
                        .header("x-forwarded-for", "198.51.100.42")
                        .body(AxumBody::from(bad_login.to_string()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        }

        // 3rd attempt within window must be blocked with 429 Too Many Requests
        let blocked_resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/login")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header("x-forwarded-for", "198.51.100.42")
                    .body(AxumBody::from(bad_login.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(blocked_resp.status(), StatusCode::TOO_MANY_REQUESTS);
    }
}
