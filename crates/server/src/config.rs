// AI-assisted
//! Environment-based server configuration (`AGENTS.md` §5.3).
//!
//! Reads all credentials and paths from `.env` and environment variables. No plain-text
//! passwords or secret hashes are ever hardcoded in source code.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
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

/// Cleans a raw value read from `.env` or environment variables:
/// trims whitespace, strips surrounding single or double quotes, and unescapes `\$` -> `$`.
pub fn sanitize_env_value(raw: &str) -> String {
    let trimmed = raw.trim();
    let unquoted = if (trimmed.starts_with('\'') && trimmed.ends_with('\'') && trimmed.len() >= 2)
        || (trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2)
    {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };
    unquoted.trim().replace("\\$", "$")
}

/// Directly parses `.env` files from candidate paths (`./.env`, `../.env`, `../../.env`)
/// so that `$argon2id$...` values with `$` signs are never corrupted by shell variable expansion.
fn parse_dotenv_map() -> HashMap<String, String> {
    let mut map = HashMap::new();
    let candidates = [
        Path::new(".env"),
        Path::new("../.env"),
        Path::new("../../.env"),
    ];

    for path in candidates {
        if let Ok(content) = fs::read_to_string(path) {
            for raw_line in content.lines() {
                let line = raw_line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let line = line.strip_prefix("export ").unwrap_or(line).trim();
                if let Some((key, val)) = line.split_once('=') {
                    let k = key.trim().to_string();
                    let v = sanitize_env_value(val);
                    if !k.is_empty() {
                        map.insert(k, v);
                    }
                }
            }
            break;
        }
    }
    map
}

fn get_config_var(dotenv_map: &HashMap<String, String>, key: &str) -> Option<String> {
    // Prefer the direct `.env` file value if present and non-empty so that an unquoted
    // shell `export RESEARCHER_PASSWORD_HASH="$argon2id..."` (which expands `$argon2id` to empty)
    // does not shadow the valid `.env` file on disk.
    if let Some(val) = dotenv_map.get(key)
        && !val.is_empty()
    {
        return Some(val.clone());
    }
    env::var(key)
        .ok()
        .map(|v| sanitize_env_value(&v))
        .filter(|v| !v.is_empty())
}

impl ServerConfig {
    /// Loads configuration from `.env` and environment variables.
    pub fn from_env() -> Self {
        let _ = dotenvy::dotenv();
        let dotenv_map = parse_dotenv_map();

        let bind_addr = get_config_var(&dotenv_map, "BIND_ADDR")
            .or_else(|| get_config_var(&dotenv_map, "SERVER_ADDR"))
            .unwrap_or_else(|| "0.0.0.0:8080".to_string());

        let data_dir = get_config_var(&dotenv_map, "DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("data/processed"));

        let weights_file = get_config_var(&dotenv_map, "WEIGHTS_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("pipeline/weights.json"));

        let static_dir = get_config_var(&dotenv_map, "STATIC_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("dist"));

        let researcher_username = get_config_var(&dotenv_map, "RESEARCHER_USERNAME")
            .unwrap_or_else(|| "equipo_analog".to_string());

        let researcher_password_hash = get_config_var(&dotenv_map, "RESEARCHER_PASSWORD_HASH");

        let session_secret = match get_config_var(&dotenv_map, "SESSION_SECRET") {
            Some(secret) => secret.into_bytes(),
            None => {
                // Generate an ephemeral per-process secret if SESSION_SECRET is unset so
                // public demo mode and local researcher mode boot cleanly without hardcoded secrets.
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

        let session_ttl_seconds = get_config_var(&dotenv_map, "SESSION_TTL_SECONDS")
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|&v| v > 0)
            .unwrap_or(28_800);

        let cookie_secure = get_config_var(&dotenv_map, "COOKIE_SECURE")
            .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
            .unwrap_or(false);

        let rate_limit_max_attempts = get_config_var(&dotenv_map, "LOGIN_RATE_LIMIT_MAX_ATTEMPTS")
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|&v| v > 0)
            .unwrap_or(20);

        let window_secs = get_config_var(&dotenv_map, "LOGIN_RATE_LIMIT_WINDOW_SECONDS")
            .and_then(|v| v.parse::<u64>().ok())
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
