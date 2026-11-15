// AI-assisted
//! Shared application state for the Axum server (`AppState`).

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, RwLock};

use analog_core::{Body, Catalog, DemExtent, SiteMetadata, WeightsConfig};

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

/// Returns a reference catalog populated with the synthetic `test_site` UV verification terrain.
pub fn default_synthetic_catalog() -> Catalog {
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
            generated_by: Some("analog_server (default_synthetic_catalog)".to_string()),
        }],
        generated_by: Some("analog_server (default_synthetic_catalog)".to_string()),
    }
}
