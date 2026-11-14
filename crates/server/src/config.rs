// AI-assisted
//! Environment-based server configuration (`AGENTS.md` §5.3).
//!
//! Reads all credentials and paths from environment variables. No plain-text passwords
//! or secret hashes are ever hardcoded in source code.

use std::env;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

/// Runtime configuration for `analog_server`.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Socket address to bind (e.g., `"0.0.0.0:8080"`).
    pub bind_addr: String,
    /// Path to `data/processed` containing `catalog.json` and site assets (`.stl`, `.tif`, `.png`).
    pub data_dir: PathBuf,
    /// Path to `pipeline/weights.json` for persisting canonical weight updates.
    pub weights_file: PathBuf,
    /// Path to the compiled Trunk WASM frontend bundle (`dist/`).
    pub static_dir: PathBuf,
    /// Researcher username expected by `POST /api/login`.
    pub researcher_username: String,
    /// Optional Argon2id PHC password hash read from `RESEARCHER_PASSWORD_HASH`.
    pub researcher_password_hash: Option<String>,
    /// Secret bytes used for HMAC-SHA256 signing of `HttpOnly` session cookies.
    pub session_secret: Vec<u8>,
    /// Session cookie lifetime in seconds.
    pub session_ttl_seconds: u64,
    /// Whether to append the `Secure` attribute to session cookies.
    pub cookie_secure: bool,
    /// Maximum allowed login attempts per IP within `rate_limit_window`.
    pub rate_limit_max_attempts: usize,
    /// Sliding window duration for `POST /api/login` rate limiting.
    pub rate_limit_window: Duration,
}

impl ServerConfig {
    /// Loads configuration from environment variables (after optional `.env` loading).
    pub fn from_env() -> Self {
        let bind_addr = env::var("SERVER_ADDR")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| "0.0.0.0:8080".to_string());

        let data_dir = env::var("DATA_DIR")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("data/processed"));

        let weights_file = env::var("WEIGHTS_FILE")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("pipeline/weights.json"));

        let static_dir = env::var("STATIC_DIR")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("dist"));

        let researcher_username = env::var("RESEARCHER_USERNAME")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| "researcher".to_string());

        let researcher_password_hash = env::var("RESEARCHER_PASSWORD_HASH")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());

        let session_secret = match env::var("SESSION_SECRET")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
        {
            Some(secret) => secret.into_bytes(),
            None => {
                // Generate an ephemeral per-process secret if SESSION_SECRET is unset so
                // public demo mode always boots cleanly without hardcoded secrets.
                let now_nanos = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos();
                let pid = std::process::id();
                let mut hasher = Sha256::new();
                hasher.update(b"analog_server_ephemeral_key:");
                hasher.update(now_nanos.to_le_bytes());
                hasher.update(pid.to_le_bytes());
                hasher.finalize().to_vec()
            }
        };

        let session_ttl_seconds = env::var("SESSION_TTL_SECONDS")
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok())
            .filter(|&v| v > 0)
            .unwrap_or(28_800);

        let cookie_secure = env::var("COOKIE_SECURE")
            .ok()
            .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
            .unwrap_or(false);

        let rate_limit_max_attempts = env::var("LOGIN_RATE_LIMIT_MAX_ATTEMPTS")
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok())
            .filter(|&v| v > 0)
            .unwrap_or(5);

        let window_secs = env::var("LOGIN_RATE_LIMIT_WINDOW_SECONDS")
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok())
            .filter(|&v| v > 0)
            .unwrap_or(60);

        Self {
            bind_addr,
            data_dir,
            weights_file,
            static_dir,
            researcher_username,
            researcher_password_hash,
            session_secret,
            session_ttl_seconds,
            cookie_secure,
            rate_limit_max_attempts,
            rate_limit_window: Duration::from_secs(window_secs),
        }
    }
}
