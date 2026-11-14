// AI-assisted
//! Shared application state for the Axum server (`AppState`).

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, RwLock};

use analog_core::{Catalog, WeightsConfig};

use crate::auth::LoginRateLimiter;
use crate::config::ServerConfig;

/// Shared state injected into all Axum route handlers.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<ServerConfig>,
    pub catalog: Arc<RwLock<Catalog>>,
    pub rate_limiter: Arc<LoginRateLimiter>,
}

impl AppState {
    /// Initializes `AppState`, loading `data/processed/catalog.json` if present on disk.
    pub fn new(config: ServerConfig) -> Self {
        let catalog_path = config.data_dir.join("catalog.json");
        let catalog = load_catalog_or_default(&catalog_path, &config.weights_file);
        let rate_limiter = Arc::new(LoginRateLimiter::new(
            config.rate_limit_max_attempts,
            config.rate_limit_window,
        ));

        Self {
            config: Arc::new(config),
            catalog: Arc::new(RwLock::new(catalog)),
            rate_limiter,
        }
    }

    /// Creates an `AppState` with an explicit initial [`Catalog`] (useful for tests).
    pub fn with_catalog(config: ServerConfig, catalog: Catalog) -> Self {
        let rate_limiter = Arc::new(LoginRateLimiter::new(
            config.rate_limit_max_attempts,
            config.rate_limit_window,
        ));

        Self {
            config: Arc::new(config),
            catalog: Arc::new(RwLock::new(catalog)),
            rate_limiter,
        }
    }

    /// Persists the current [`Catalog`] to `data/processed/catalog.json` (best effort if directory exists).
    pub fn persist_catalog(&self, catalog: &Catalog) -> Result<(), String> {
        let catalog_path = self.config.data_dir.join("catalog.json");
        if let Some(parent) = catalog_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create data directory {}: {e}", parent.display()))?;
        }
        let json = serde_json::to_string_pretty(catalog)
            .map_err(|e| format!("Failed to serialize catalog.json: {e}"))?;
        fs::write(&catalog_path, format!("{json}\n"))
            .map_err(|e| format!("Failed to write {}: {e}", catalog_path.display()))
    }

    /// Persists updated canonical weights to `pipeline/weights.json`.
    pub fn persist_weights(&self, weights: &BTreeMap<String, f64>) -> Result<(), String> {
        let weights_cfg = WeightsConfig {
            weights: weights.clone(),
            generated_by: Some("analog_server (authenticated researcher)".to_string()),
        };
        if let Some(parent) = self.config.weights_file.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(|e| {
                format!(
                    "Failed to create weights directory {}: {e}",
                    parent.display()
                )
            })?;
        }
        let json = serde_json::to_string_pretty(&weights_cfg)
            .map_err(|e| format!("Failed to serialize weights.json: {e}"))?;
        fs::write(&self.config.weights_file, format!("{json}\n"))
            .map_err(|e| format!("Failed to write {}: {e}", self.config.weights_file.display()))
    }
}

fn load_catalog_or_default(catalog_path: &Path, weights_path: &Path) -> Catalog {
    if let Ok(raw) = fs::read_to_string(catalog_path)
        && let Ok(catalog) = serde_json::from_str::<Catalog>(&raw)
    {
        return catalog;
    }

    let default_weights = fs::read_to_string(weights_path)
        .ok()
        .and_then(|raw| serde_json::from_str::<WeightsConfig>(&raw).ok())
        .map(|w| w.weights)
        .unwrap_or_default();

    Catalog {
        version: "1.0.0".to_string(),
        default_weights,
        sites: Vec::new(),
        generated_by: Some("analog_server (fallback default catalog)".to_string()),
    }
}
