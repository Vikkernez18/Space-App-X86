// AI-assisted
//! Asynchronous HTTP client bridge (`ehttp`) and browser file picker / drag-and-drop bridge
//! for WASM and native desktop.
//!
//! Connects the Bevy viewer to:
//! - Public Demo Mode (`GET /api/demo`, `GET /api/catalog`, `POST /api/recalculate`)
//! - Static fallback (`data/processed/catalog.json`, `.stl`, `.tif`, `.png`) for GitHub Pages
//! - Argon2id Researcher Authentication (`POST /api/login`, `POST /api/logout`, `GET /api/me`)
//!   with both `HttpOnly; SameSite=Lax` cookies and `X-Analog-Session` token header support
//! - Protected Researcher Actions (`PUT /api/weights`, `POST /api/sites`, `POST /api/sites/upload`)
//!   using raw binary `multipart/form-data` transport (zero Base64 in memory).

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

use analog_core::terrain_analysis::{
    UploadSiteRequest, UploadSiteResponse, encode_multipart_upload_request,
};
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

/// File selected via browser `<input type="file">`, HTML5 drag-and-drop, or native file drop.
/// Stores raw binary `Vec<u8>` bytes directly without Base64 expansion.
#[derive(Debug, Clone)]
pub struct BrowserSelectedFile {
    pub name: String,
    pub size: usize,
    pub bytes: Vec<u8>,
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
    SiteUploaded(UploadSiteResponse),
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
    pub session_token: Option<String>,
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
    session_token: Arc<Mutex<Option<String>>>,
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
            session_token: Arc::new(Mutex::new(None)),
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
            format!("/{clean}")
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

    fn attach_auth(&self, req: &mut ehttp::Request) {
        if let Some(token) = self
            .session_token
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
        {
            req.headers.insert("x-analog-session", token);
        }
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

    /// Queries `GET /api/me` to check if an authenticated researcher session is active.
    pub fn check_session(&self) {
        let mut req = ehttp::Request::get(self.api_url("api/me"));
        self.attach_auth(&mut req);
        let queue = Arc::clone(&self.queue);
        let token_store = Arc::clone(&self.session_token);

        ehttp::fetch(req, move |result| {
            if let Ok(resp) = result
                && resp.ok
                && let Ok(status) = serde_json::from_slice::<SessionApiPayload>(&resp.bytes)
            {
                if let Some(tok) = &status.session_token {
                    *token_store.lock().unwrap_or_else(|p| p.into_inner()) = Some(tok.clone());
                }
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
        let token_store = Arc::clone(&self.session_token);
        let bridge_after_login = self.clone();

        ehttp::fetch(req, move |result| match result {
            Ok(resp) if resp.ok => {
                Self::capture_cookie_from_response(&cookie_store, &resp);
                if let Ok(status) = serde_json::from_slice::<SessionApiPayload>(&resp.bytes) {
                    if let Some(tok) = &status.session_token {
                        *token_store.lock().unwrap_or_else(|p| p.into_inner()) =
                            Some(tok.clone());
                    }
                    Self::push_to_queue(
                        &queue,
                        NetEvent::SessionStatus {
                            authenticated: status.authenticated,
                            username: status.username,
                            mode: status.mode,
                            message: status.message,
                        },
                    );
                    if status.authenticated {
                        bridge_after_login.fetch_catalog_with_static_fallback();
                    }
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

    /// Submits `POST /api/logout` to clear the researcher session cookie and token.
    pub fn logout(&self) {
        let mut req = ehttp::Request::post(self.api_url("api/logout"), Vec::new());
        self.attach_auth(&mut req);
        let queue = Arc::clone(&self.queue);
        let cookie_store = Arc::clone(&self.session_cookie);
        let token_store = Arc::clone(&self.session_token);

        ehttp::fetch(req, move |result| {
            *cookie_store.lock().unwrap_or_else(|p| p.into_inner()) = None;
            *token_store.lock().unwrap_or_else(|p| p.into_inner()) = None;
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
        self.attach_auth(&mut req);

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
        self.attach_auth(&mut req);

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
        self.attach_auth(&mut req);

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

    /// Sends `POST /api/sites/upload` (authenticated researcher only) using raw binary
    /// `multipart/form-data` (zero Base64 encoding) to upload `.stl`, `.tif`/`.tiff`,
    /// and/or `.png` files, automatically decimate large meshes (`> 50,000` triangles),
    /// run morphometric + topological data analysis (0D Persistent Homology + Euler characteristic),
    /// and register the new site in `data/processed/`.
    pub fn upload_and_analyze_site(&self, upload_req: UploadSiteRequest) {
        let (content_type, body) = encode_multipart_upload_request(&upload_req);
        let mut req = ehttp::Request::post(self.api_url("api/sites/upload"), body);
        req.headers.insert("Content-Type", content_type);
        self.attach_auth(&mut req);

        let queue = Arc::clone(&self.queue);
        ehttp::fetch(req, move |result| match result {
            Ok(resp) if resp.ok => match serde_json::from_slice::<UploadSiteResponse>(&resp.bytes)
            {
                Ok(uploaded) => {
                    Self::push_to_queue(&queue, NetEvent::SiteUploaded(uploaded));
                }
                Err(err) => {
                    Self::push_to_queue(
                        &queue,
                        NetEvent::Error(format!("Failed to parse upload analysis response: {err}")),
                    );
                }
            },
            Ok(resp) => {
                Self::push_to_queue(&queue, NetEvent::Error(Self::parse_api_error(&resp)));
            }
            Err(err) => {
                Self::push_to_queue(
                    &queue,
                    NetEvent::Error(format!("Upload & terrain analysis request failed: {err}")),
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

/// Installs an HTML5 drag-and-drop file reader on `window` when running in WebAssembly.
/// Uses `FileReader.readAsArrayBuffer` + `Uint8Array` so 50–100+ MiB files are read as raw
/// binary bytes without Base64 string expansion or JSON serialization overhead.
pub fn install_browser_file_drop_listener() {
    #[cfg(target_arch = "wasm32")]
    {
        let script = r#"
        (function() {
            if (window.__space_app_drop_installed) return;
            window.__space_app_drop_installed = true;
            window.__space_app_uploads = window.__space_app_uploads || [];
            window.__space_app_read_files = function(fileList) {
                if (!fileList) return;
                for (let i = 0; i < fileList.length; i++) {
                    const f = fileList[i];
                    const reader = new FileReader();
                    reader.onload = function(evt) {
                        const buf = (evt.target && evt.target.result)
                            ? new Uint8Array(evt.target.result)
                            : new Uint8Array(0);
                        window.__space_app_uploads.push({
                            name: String(f.name || "uploaded.bin"),
                            size: Number(f.size || buf.byteLength || 0),
                            bytes: buf
                        });
                    };
                    reader.readAsArrayBuffer(f);
                }
            };
            window.addEventListener("dragover", function(e) {
                e.preventDefault();
            }, false);
            window.addEventListener("drop", function(e) {
                e.preventDefault();
                if (e.dataTransfer && e.dataTransfer.files) {
                    window.__space_app_read_files(e.dataTransfer.files);
                }
            }, false);
        })();
        "#;
        let _ = js_sys::eval(script);
    }
}

/// Opens a browser `<input type="file">` dialog for selecting `.stl`, `.tif`/`.tiff`, or `.png` files.
#[allow(unused_variables)]
pub fn open_browser_file_dialog(accept: &str) {
    #[cfg(target_arch = "wasm32")]
    {
        install_browser_file_drop_listener();
        let escaped_accept = accept.replace('"', "");
        let script = format!(
            r#"
            (function() {{
                let inp = document.createElement("input");
                inp.type = "file";
                inp.multiple = true;
                inp.accept = "{escaped_accept}";
                inp.style.display = "none";
                inp.onchange = function(e) {{
                    if (inp.files && window.__space_app_read_files) {{
                        window.__space_app_read_files(inp.files);
                    }}
                    inp.remove();
                }};
                document.body.appendChild(inp);
                inp.click();
            }})();
            "#
        );
        let _ = js_sys::eval(&script);
    }
}

/// Drains any files selected or dropped in the browser since the last frame, transferring raw
/// `Uint8Array` buffers into Rust `Vec<u8>` via `js_sys::Uint8Array` without Base64/JSON.
pub fn poll_browser_uploaded_files() -> Vec<BrowserSelectedFile> {
    #[cfg(target_arch = "wasm32")]
    {
        let mut out = Vec::new();
        loop {
            let has_next = js_sys::eval(
                "Boolean(window.__space_app_uploads && window.__space_app_uploads.length > 0)",
            )
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
            if !has_next {
                break;
            }

            let _ = js_sys::eval(
                "window.__space_app_current_file = window.__space_app_uploads.shift();",
            );
            let name = js_sys::eval(
                "window.__space_app_current_file ? String(window.__space_app_current_file.name || '') : ''",
            )
            .ok()
            .and_then(|v| v.as_string())
            .unwrap_or_default();

            let bytes = js_sys::eval(
                "window.__space_app_current_file && window.__space_app_current_file.bytes ? window.__space_app_current_file.bytes : new Uint8Array(0)",
            )
            .ok()
            .map(|val| js_sys::Uint8Array::from(val).to_vec())
            .unwrap_or_default();

            let _ = js_sys::eval("window.__space_app_current_file = null;");

            if !name.is_empty() {
                let size = bytes.len();
                out.push(BrowserSelectedFile { name, size, bytes });
            }
        }
        return out;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        Vec::new()
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
            triangle_count: 8192,
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
