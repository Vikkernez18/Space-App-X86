// AI-assisted
//! Asynchronous HTTP client bridge (`ehttp`) for WASM and native desktop.
//!
//! Connects the Bevy viewer to:
//! - Public Demo Mode (`GET /api/demo`, `GET /api/catalog`, `POST /api/recalculate`)
//! - Static fallback (`data/processed/catalog.json`, `.stl`, `.tif`, `.png`) for GitHub Pages
//! - Argon2id Researcher Authentication (`POST /api/login`, `POST /api/logout`, `GET /api/me`)
//! - Protected Researcher Actions (`PUT /api/weights`, `POST /api/sites`)

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

use analog_core::{
    Body, Catalog, DemExtent, RecalculateRequest, RecalculateResponse, SiteMetadata,
};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Distinguishes the primary terrain viewport from the optional side-by-side comparison terrain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TerrainSlot {
    Primary,
    Comparison,
}

/// Events produced by asynchronous HTTP / asset callbacks and consumed by Bevy `Update` systems.
#[derive(Debug, Clone)]
pub enum NetEvent {
    DemoLoaded {
        catalog: Catalog,
        message: String,
    },
    CatalogLoaded(Catalog),
    SessionStatus {
        authenticated: bool,
        username: Option<String>,
        mode: String,
        message: String,
    },
    Recalculated(RecalculateResponse),
    CanonicalWeightsSaved(Catalog),
    SiteSaved(SiteMetadata),
    SiteMeshBytesLoaded {
        slot: TerrainSlot,
        site_id: String,
        bytes: Vec<u8>,
    },
    SiteTextureBytesLoaded {
        slot: TerrainSlot,
        site_id: String,
        bytes: Vec<u8>,
    },
    Error(String),
}

#[derive(Debug, Clone, Deserialize)]
struct DemoApiPayload {
    pub message: String,
    pub catalog: Catalog,
}

#[derive(Debug, Clone, Deserialize)]
struct SessionApiPayload {
    pub authenticated: bool,
    pub mode: String,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ErrorApiPayload {
    pub error: String,
}

#[derive(Debug, Clone, Serialize)]
struct LoginApiPayload {
    pub username: String,
    pub password: String,
}

/// Bevy resource managing non-blocking HTTP requests and the incoming [`NetEvent`] queue.
#[derive(Resource, Clone)]
pub struct NetBridge {
    queue: Arc<Mutex<VecDeque<NetEvent>>>,
    session_cookie: Arc<Mutex<Option<String>>>,
    base_url: String,
}

impl Default for NetBridge {
    fn default() -> Self {
        let base_url = if cfg!(target_arch = "wasm32") {
            String::new()
        } else {
            std::env::var("SERVER_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".to_string())
        };

        Self {
            queue: Arc::new(Mutex::new(VecDeque::new())),
            session_cookie: Arc::new(Mutex::new(None)),
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }
}

impl NetBridge {
    /// Drains all pending [`NetEvent`] messages received since the last frame.
    pub fn drain_events(&self) -> Vec<NetEvent> {
        let mut guard = self
            .queue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.drain(..).collect()
    }

    pub fn push_event(&self, event: NetEvent) {
        Self::push_to_queue(&self.queue, event);
    }

    fn push_to_queue(queue: &Arc<Mutex<VecDeque<NetEvent>>>, event: NetEvent) {
        let mut guard = queue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.push_back(event);
    }

    fn api_url(&self, path: &str) -> String {
        let clean = path.trim_start_matches('/');
        if self.base_url.is_empty() {
            clean.to_string()
        } else {
            format!("{}/{}", self.base_url, clean)
        }
    }

    fn asset_urls(&self, relative_data_path: &str) -> (String, String) {
        let clean = relative_data_path
            .trim_start_matches('/')
            .trim_start_matches("data/processed/");
        if self.base_url.is_empty() {
            (
                format!("/data/processed/{clean}"),
                format!("data/processed/{clean}"),
            )
        } else {
            (
                format!("{}/data/processed/{clean}", self.base_url),
                format!("data/processed/{clean}"),
            )
        }
    }

    fn attach_cookie(&self, req: &mut ehttp::Request) {
        if let Some(cookie) = self
            .session_cookie
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
        {
            req.headers.insert("Cookie", cookie);
        }
    }

    fn capture_cookie_from_response(
        cookie_store: &Arc<Mutex<Option<String>>>,
        resp: &ehttp::Response,
    ) {
        if let Some(set_cookie) = resp.headers.get("set-cookie")
            && let Some(pair) = set_cookie.split(';').next()
        {
            let trimmed = pair.trim();
            if trimmed.ends_with('=') {
                *cookie_store.lock().unwrap_or_else(|p| p.into_inner()) = None;
            } else if !trimmed.is_empty() {
                *cookie_store.lock().unwrap_or_else(|p| p.into_inner()) =
                    Some(trimmed.to_string());
            }
        }
    }

    fn parse_api_error(resp: &ehttp::Response) -> String {
        if let Ok(parsed) = serde_json::from_slice::<ErrorApiPayload>(&resp.bytes) {
            format!("HTTP {}: {}", resp.status, parsed.error)
        } else {
            let text = resp.text().unwrap_or("Unknown server error");
            format!("HTTP {}: {}", resp.status, text)
        }
    }

    /// Queries `GET /api/me` to check if an authenticated researcher session cookie is active.
    pub fn check_session(&self) {
        let mut req = ehttp::Request::get(self.api_url("api/me"));
        self.attach_cookie(&mut req);
        let queue = Arc::clone(&self.queue);

        ehttp::fetch(req, move |result| {
            if let Ok(resp) = result
                && resp.ok
                && let Ok(status) = serde_json::from_slice::<SessionApiPayload>(&resp.bytes)
            {
                Self::push_to_queue(
                    &queue,
                    NetEvent::SessionStatus {
                        authenticated: status.authenticated,
                        username: status.username,
                        mode: status.mode,
                        message: status.message,
                    },
                );
            }
        });
    }

    /// Enters Public Demo Mode without a password by querying `GET /api/demo`,
    /// automatically falling back to `GET /api/catalog` or static `data/processed/catalog.json`
    /// (for static GitHub Pages deployments).
    pub fn fetch_demo_mode(&self) {
        let req = ehttp::Request::get(self.api_url("api/demo"));
        let queue = Arc::clone(&self.queue);
        let fallback_bridge = self.clone();

        ehttp::fetch(req, move |result| match result {
            Ok(resp) if resp.ok => {
                if let Ok(demo) = serde_json::from_slice::<DemoApiPayload>(&resp.bytes) {
                    Self::push_to_queue(
                        &queue,
                        NetEvent::DemoLoaded {
                            catalog: demo.catalog,
                            message: demo.message,
                        },
                    );
                } else {
                    fallback_bridge.fetch_catalog_with_static_fallback();
                }
            }
            _ => {
                fallback_bridge.fetch_catalog_with_static_fallback();
            }
        });
    }

    /// Fetches `GET /api/catalog`, falling back to static `data/processed/catalog.json`
    /// or the built-in synthetic `test_site` catalog if running offline.
    pub fn fetch_catalog_with_static_fallback(&self) {
        let req = ehttp::Request::get(self.api_url("api/catalog"));
        let queue = Arc::clone(&self.queue);
        let (_, static_url) = self.asset_urls("catalog.json");

        ehttp::fetch(req, move |result| {
            if let Ok(resp) = result
                && resp.ok
                && let Ok(catalog) = serde_json::from_slice::<Catalog>(&resp.bytes)
            {
                Self::push_to_queue(&queue, NetEvent::CatalogLoaded(catalog));
                return;
            }

            let static_req = ehttp::Request::get(static_url);
            let queue_inner = Arc::clone(&queue);
            ehttp::fetch(static_req, move |static_res| {
                if let Ok(resp) = static_res
                    && resp.ok
                    && let Ok(catalog) = serde_json::from_slice::<Catalog>(&resp.bytes)
                {
                    Self::push_to_queue(
                        &queue_inner,
                        NetEvent::DemoLoaded {
                            catalog,
                            message: "Static Public Demo Mode active (loaded data/processed/catalog.json)."
                                .to_string(),
                        },
                    );
                } else {
                    Self::push_to_queue(
                        &queue_inner,
                        NetEvent::DemoLoaded {
                            catalog: fallback_synthetic_catalog(),
                            message: "Public Demo Mode active (using built-in synthetic test_site)."
                                .to_string(),
                        },
                    );
                }
            });
        });
    }

    /// Submits `POST /api/login` with Argon2id researcher credentials.
    pub fn login(&self, username: String, password: String) {
        let payload = LoginApiPayload { username, password };
        let body = serde_json::to_vec(&payload).unwrap_or_default();
        let mut req = ehttp::Request::post(self.api_url("api/login"), body);
        req.headers.insert("Content-Type", "application/json");

        let queue = Arc::clone(&self.queue);
        let cookie_store = Arc::clone(&self.session_cookie);

        ehttp::fetch(req, move |result| match result {
            Ok(resp) if resp.ok => {
                Self::capture_cookie_from_response(&cookie_store, &resp);
                if let Ok(status) = serde_json::from_slice::<SessionApiPayload>(&resp.bytes) {
                    Self::push_to_queue(
                        &queue,
                        NetEvent::SessionStatus {
                            authenticated: status.authenticated,
                            username: status.username,
                            mode: status.mode,
                            message: status.message,
                        },
                    );
                }
            }
            Ok(resp) => {
                Self::push_to_queue(&queue, NetEvent::Error(Self::parse_api_error(&resp)));
            }
            Err(err) => {
                Self::push_to_queue(
                    &queue,
                    NetEvent::Error(format!("Login request failed: {err}")),
                );
            }
        });
    }

    /// Submits `POST /api/logout` to clear the researcher session cookie.
    pub fn logout(&self) {
        let mut req = ehttp::Request::post(self.api_url("api/logout"), Vec::new());
        self.attach_cookie(&mut req);
        let queue = Arc::clone(&self.queue);
        let cookie_store = Arc::clone(&self.session_cookie);

        ehttp::fetch(req, move |result| {
            *cookie_store.lock().unwrap_or_else(|p| p.into_inner()) = None;
            if let Ok(resp) = result
                && resp.ok
                && let Ok(status) = serde_json::from_slice::<SessionApiPayload>(&resp.bytes)
            {
                Self::push_to_queue(
                    &queue,
                    NetEvent::SessionStatus {
                        authenticated: false,
                        username: None,
                        mode: status.mode,
                        message: status.message,
                    },
                );
            } else {
                Self::push_to_queue(
                    &queue,
                    NetEvent::SessionStatus {
                        authenticated: false,
                        username: None,
                        mode: "demo".to_string(),
                        message: "Logged out. Public Demo Mode active.".to_string(),
                    },
                );
            }
        });
    }

    /// Sends `POST /api/recalculate` with custom feature weights.
    pub fn recalculate_scores(&self, weights: BTreeMap<String, f64>) {
        let payload = RecalculateRequest { weights };
        let body = serde_json::to_vec(&payload).unwrap_or_default();
        let mut req = ehttp::Request::post(self.api_url("api/recalculate"), body);
        req.headers.insert("Content-Type", "application/json");
        self.attach_cookie(&mut req);

        let queue = Arc::clone(&self.queue);
        ehttp::fetch(req, move |result| {
            if let Ok(resp) = result
                && resp.ok
                && let Ok(recalc) = serde_json::from_slice::<RecalculateResponse>(&resp.bytes)
            {
                Self::push_to_queue(&queue, NetEvent::Recalculated(recalc));
            }
        });
    }

    /// Sends `PUT /api/weights` (authenticated researcher only) to persist canonical weights.
    pub fn save_canonical_weights(&self, weights: BTreeMap<String, f64>) {
        let payload = RecalculateRequest { weights };
        let body = serde_json::to_vec(&payload).unwrap_or_default();
        let mut req = ehttp::Request::post(self.api_url("api/weights"), body);
        req.method = "PUT".to_string();
        req.headers.insert("Content-Type", "application/json");
        self.attach_cookie(&mut req);

        let queue = Arc::clone(&self.queue);
        ehttp::fetch(req, move |result| match result {
            Ok(resp) if resp.ok => {
                if let Ok(cat) = serde_json::from_slice::<Catalog>(&resp.bytes) {
                    Self::push_to_queue(&queue, NetEvent::CanonicalWeightsSaved(cat));
                }
            }
            Ok(resp) => {
                Self::push_to_queue(&queue, NetEvent::Error(Self::parse_api_error(&resp)));
            }
            Err(err) => {
                Self::push_to_queue(
                    &queue,
                    NetEvent::Error(format!("Failed to save canonical weights: {err}")),
                );
            }
        });
    }

    /// Sends `POST /api/sites` (authenticated researcher only) to register or update a site.
    pub fn save_site(&self, site: SiteMetadata) {
        let body = serde_json::to_vec(&site).unwrap_or_default();
        let mut req = ehttp::Request::post(self.api_url("api/sites"), body);
        req.headers.insert("Content-Type", "application/json");
        self.attach_cookie(&mut req);

        let queue = Arc::clone(&self.queue);
        ehttp::fetch(req, move |result| match result {
            Ok(resp) if resp.ok => {
                if let Ok(saved) = serde_json::from_slice::<SiteMetadata>(&resp.bytes) {
                    Self::push_to_queue(&queue, NetEvent::SiteSaved(saved));
                }
            }
            Ok(resp) => {
                Self::push_to_queue(&queue, NetEvent::Error(Self::parse_api_error(&resp)));
            }
            Err(err) => {
                Self::push_to_queue(
                    &queue,
                    NetEvent::Error(format!("Failed to save site metadata: {err}")),
                );
            }
        });
    }

    /// Fetches a site's terrain mesh (`.stl` or `.tif`/`.tiff`) and `.png` texture from `/data/processed/`.
    pub fn fetch_site_assets(&self, slot: TerrainSlot, site: &SiteMetadata) {
        let site_id = site.id.clone();
        let (mesh_primary_url, mesh_fallback_url) = self.asset_urls(&site.mesh_path);
        let queue_mesh = Arc::clone(&self.queue);

        let req_mesh = ehttp::Request::get(mesh_primary_url);
        ehttp::fetch(req_mesh, move |result| {
            if let Ok(resp) = result
                && resp.ok
                && !resp.bytes.is_empty()
            {
                Self::push_to_queue(
                    &queue_mesh,
                    NetEvent::SiteMeshBytesLoaded {
                        slot,
                        site_id,
                        bytes: resp.bytes,
                    },
                );
                return;
            }

            let req_fallback = ehttp::Request::get(mesh_fallback_url);
            let queue_inner = Arc::clone(&queue_mesh);
            ehttp::fetch(req_fallback, move |res2| {
                if let Ok(resp2) = res2
                    && resp2.ok
                    && !resp2.bytes.is_empty()
                {
                    Self::push_to_queue(
                        &queue_inner,
                        NetEvent::SiteMeshBytesLoaded {
                            slot,
                            site_id,
                            bytes: resp2.bytes,
                        },
                    );
                }
            });
        });

        let site_id_tex = site.id.clone();
        let (tex_primary_url, tex_fallback_url) = self.asset_urls(&site.texture_path);
        let queue_tex = Arc::clone(&self.queue);

        let req_tex = ehttp::Request::get(tex_primary_url);
        ehttp::fetch(req_tex, move |result| {
            if let Ok(resp) = result
                && resp.ok
                && !resp.bytes.is_empty()
            {
                Self::push_to_queue(
                    &queue_tex,
                    NetEvent::SiteTextureBytesLoaded {
                        slot,
                        site_id: site_id_tex,
                        bytes: resp.bytes,
                    },
                );
                return;
            }

            let req_fallback = ehttp::Request::get(tex_fallback_url);
            let queue_inner = Arc::clone(&queue_tex);
            ehttp::fetch(req_fallback, move |res2| {
                if let Ok(resp2) = res2
                    && resp2.ok
                    && !resp2.bytes.is_empty()
                {
                    Self::push_to_queue(
                        &queue_inner,
                        NetEvent::SiteTextureBytesLoaded {
                            slot,
                            site_id: site_id_tex,
                            bytes: resp2.bytes,
                        },
                    );
                }
            });
        });
    }
}

/// Built-in synthetic fallback catalog containing `test_site` so the viewer works
/// even before `catalog.json` or the backend server is reachable.
pub fn fallback_synthetic_catalog() -> Catalog {
    let mut default_weights = BTreeMap::new();
    default_weights.insert("slope_safety".to_string(), 0.35);
    default_weights.insert("roughness_similarity".to_string(), 0.25);
    default_weights.insert("elevation_relief".to_string(), 0.20);
    default_weights.insert("geomorphology_match".to_string(), 0.20);

    let mut feature_scores = BTreeMap::new();
    feature_scores.insert("slope_safety".to_string(), 0.85);
    feature_scores.insert("roughness_similarity".to_string(), 0.90);
    feature_scores.insert("elevation_relief".to_string(), 0.75);
    feature_scores.insert("geomorphology_match".to_string(), 0.88);

    Catalog {
        version: "1.0.0".to_string(),
        default_weights,
        sites: vec![SiteMetadata {
            id: "test_site".to_string(),
            name: "Synthetic Crater & Ridge UV Verification Site".to_string(),
            body: Body::Moon,
            description: "Synthetic lunar crater and ridge terrain in Z-up meter coordinates used to verify DemExtent planar UV alignment against an 8x8 checkerboard texture.".to_string(),
            center_lat_lon: Some([-89.5, 0.0]),
            dem_extent: DemExtent::new(-1000.0, 1000.0, -1000.0, 1000.0),
            elevation_range_m: Some([-45.0, 120.0]),
            triangle_count: 2048,
            mesh_path: "test_site/terrain.stl".to_string(),
            mesh_format: None,
            texture_path: "test_site/checkerboard.png".to_string(),
            feature_scores,
            canonical_score: 0.8485,
            generated_by: Some("analog_viewer fallback".to_string()),
        }],
        generated_by: Some("analog_viewer fallback".to_string()),
    }
}
