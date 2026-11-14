// AI-assisted
//! Argon2id researcher authentication, HMAC-SHA256 signed `HttpOnly` / `SameSite=Lax`
//! session cookies, and sliding-window login rate limiting.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use axum::http::{HeaderMap, header};
use sha2::{Digest, Sha256};

/// Name of the researcher session cookie.
pub const SESSION_COOKIE_NAME: &str = "analog_session";

/// Verifies a plain-text password against an Argon2id PHC string (`$argon2id$...`).
pub fn verify_argon2_password(password: &str, expected_phc_hash: &str) -> Result<bool, String> {
    let parsed_hash = PasswordHash::new(expected_phc_hash.trim())
        .map_err(|e| format!("Invalid Argon2 PHC password hash configuration: {e}"))?;

    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok())
}

/// Generates an Argon2id PHC password hash string (`$argon2id$v=19$...`) for `.env` setup
/// and automated integration tests.
pub fn hash_argon2_password(password: &str, salt_seed: &[u8]) -> Result<String, String> {
    let mut hasher = Sha256::new();
    hasher.update(b"analog_server_argon2_salt:");
    hasher.update(salt_seed);
    let digest = hasher.finalize();

    // Use the first 16 bytes of the SHA-256 digest as the 128-bit Argon2 salt.
    let salt = SaltString::encode_b64(&digest[..16])
        .map_err(|e| format!("Failed to encode Argon2 salt: {e}"))?;

    let phc = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| format!("Failed to hash password with Argon2id: {e}"))?
        .to_string();

    Ok(phc)
}

/// Computes RFC 2104 HMAC-SHA256.
fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    const BLOCK_SIZE: usize = 64;
    let mut key_block = [0u8; BLOCK_SIZE];
    if key.len() > BLOCK_SIZE {
        let mut kh = Sha256::new();
        kh.update(key);
        let hashed = kh.finalize();
        key_block[..32].copy_from_slice(&hashed);
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }

    let mut ipad = [0x36u8; BLOCK_SIZE];
    let mut opad = [0x5cu8; BLOCK_SIZE];
    for i in 0..BLOCK_SIZE {
        ipad[i] ^= key_block[i];
        opad[i] ^= key_block[i];
    }

    let mut inner = Sha256::new();
    inner.update(ipad);
    inner.update(message);
    let inner_hash = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(inner_hash);
    let outer_hash = outer.finalize();

    let mut out = [0u8; 32];
    out.copy_from_slice(&outer_hash);
    out
}

/// Constant-time byte slice equality check to prevent timing attacks.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&x, &y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn current_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Creates a signed session token for `username` expiring after `ttl_seconds`.
pub fn create_session_token(username: &str, secret: &[u8], ttl_seconds: u64) -> String {
    let exp = current_unix_seconds().saturating_add(ttl_seconds);
    let payload = format!("{username}.{exp}");
    let sig = hex_encode(&hmac_sha256(secret, payload.as_bytes()));
    format!("{payload}.{sig}")
}

/// Verifies a signed session token and returns the authenticated username if valid and unexpired.
pub fn verify_session_token(
    token: &str,
    secret: &[u8],
    expected_username: &str,
) -> Option<String> {
    let mut parts = token.trim().splitn(3, '.');
    let username = parts.next()?;
    let exp_str = parts.next()?;
    let sig_hex = parts.next()?;

    if username != expected_username {
        return None;
    }

    let exp = exp_str.parse::<u64>().ok()?;
    if current_unix_seconds() > exp {
        return None;
    }

    let payload = format!("{username}.{exp}");
    let expected_sig = hex_encode(&hmac_sha256(secret, payload.as_bytes()));
    if !constant_time_eq(sig_hex.as_bytes(), expected_sig.as_bytes()) {
        return None;
    }

    Some(username.to_string())
}

/// Builds a `Set-Cookie` header value with `HttpOnly` and `SameSite=Lax` attributes.
pub fn build_session_cookie_header(
    username: &str,
    secret: &[u8],
    ttl_seconds: u64,
    secure: bool,
) -> String {
    let token = create_session_token(username, secret, ttl_seconds);
    let secure_attr = if secure { "; Secure" } else { "" };
    format!(
        "{SESSION_COOKIE_NAME}={token}; HttpOnly; SameSite=Lax; Path=/; Max-Age={ttl_seconds}{secure_attr}"
    )
}

/// Builds a `Set-Cookie` header value that immediately expires the session cookie.
pub fn build_logout_cookie_header(secure: bool) -> String {
    let secure_attr = if secure { "; Secure" } else { "" };
    format!("{SESSION_COOKIE_NAME}=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0{secure_attr}")
}

/// Extracts and validates the researcher session cookie from incoming HTTP headers.
pub fn extract_authenticated_researcher(
    headers: &HeaderMap,
    secret: &[u8],
    expected_username: &str,
) -> Option<String> {
    for cookie_header in headers.get_all(header::COOKIE) {
        let Ok(raw_str) = cookie_header.to_str() else {
            continue;
        };
        for pair in raw_str.split(';') {
            let trimmed = pair.trim();
            if let Some((name, val)) = trimmed.split_once('=')
                && name.trim() == SESSION_COOKIE_NAME
                && let Some(user) = verify_session_token(val.trim(), secret, expected_username)
            {
                return Some(user);
            }
        }
    }
    None
}

/// Extracts a client identifier (IP string) from headers for login rate limiting.
pub fn extract_client_key(headers: &HeaderMap) -> String {
    if let Some(xff) = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return xff.to_string();
    }
    if let Some(real_ip) = headers
        .get("x-real-ip")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return real_ip.to_string();
    }
    "default-client".to_string()
}

/// Thread-safe sliding-window rate limiter for `POST /api/login`.
#[derive(Debug)]
pub struct LoginRateLimiter {
    max_attempts: usize,
    window: Duration,
    buckets: Mutex<HashMap<String, Vec<Instant>>>,
}

impl LoginRateLimiter {
    /// Creates a new rate limiter allowing `max_attempts` per `window`.
    pub fn new(max_attempts: usize, window: Duration) -> Self {
        Self {
            max_attempts: max_attempts.max(1),
            window,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    /// Checks whether `client_key` is allowed to attempt login now.
    /// Returns `Ok(())` and records the attempt if within budget, or `Err(retry_after_seconds)`
    /// when rate-limited.
    pub fn check_and_record(&self, client_key: &str) -> Result<(), u64> {
        let now = Instant::now();
        let mut guard = self
            .buckets
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let attempts = guard.entry(client_key.to_string()).or_default();
        let window = self.window;
        attempts.retain(|&ts| now.duration_since(ts) < window);

        if attempts.len() >= self.max_attempts {
            let oldest = attempts.first().copied().unwrap_or(now);
            let elapsed = now.duration_since(oldest);
            let remaining = window.saturating_sub(elapsed).as_secs().max(1);
            return Err(remaining);
        }

        attempts.push(now);
        Ok(())
    }
}
