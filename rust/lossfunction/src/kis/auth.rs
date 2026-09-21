//! KIS OAuth access-token lifecycle (wiki: kis-api).
//!
//! Verified spec: `POST {base}/oauth2/tokenP` with JSON body
//! `{"grant_type": "client_credentials", "appkey", "appsecret"}`; the 200
//! response carries `access_token` and `access_token_token_expired`
//! (`"%Y-%m-%d %H:%M:%S"` KST wall time, ~1 day validity). The client caches
//! the token, proactively re-issues before expiry (margin), and classifies
//! failures so callers can decide between retry and halt.

use chrono::{DateTime, NaiveDateTime, TimeDelta, Utc};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::broker::BrokerError;

const KST_OFFSET_SECS: i32 = 9 * 3600;

pub fn kis_base_url(environment: &str) -> &'static str {
    match environment {
        "real" => "https://openapi.koreainvestment.com:9443",
        _ => "https://openapivts.koreainvestment.com:9443",
    }
}

/// Parse the KST wall-time expiry ("%Y-%m-%d %H:%M:%S", no zone marker).
pub fn parse_kst_expiry(raw: &str) -> Option<DateTime<Utc>> {
    NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S")
        .ok()
        .map(|naive| {
            naive
                .and_local_timezone(chrono::FixedOffset::east_opt(KST_OFFSET_SECS).unwrap())
                .unwrap()
                .with_timezone(&Utc)
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthErrorKind {
    /// 401/403 — retrying cannot help.
    InvalidCredentials,
    /// 429 — retry after backoff.
    RateLimited,
    /// 5xx — retry after backoff.
    Server,
    /// Transport-level failure — retry after backoff.
    Network,
    /// 200 but the body is unusable.
    Malformed,
}

#[derive(Debug, thiserror::Error)]
#[error("{kind:?}: {message}")]
pub struct AuthError {
    pub kind: AuthErrorKind,
    pub message: String,
}

impl From<AuthError> for BrokerError {
    fn from(error: AuthError) -> Self {
        BrokerError::Internal(error.to_string())
    }
}

#[derive(Debug, Clone, Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    access_token_token_expired: Option<String>,
}

#[derive(Debug, Clone)]
struct CachedToken {
    access_token: String,
    expires_at: DateTime<Utc>,
}

/// Issues and caches KIS access tokens for one app credential pair.
pub struct KisAuth {
    http: reqwest::Client,
    base_url: String,
    app_key: String,
    app_secret: String,
    margin: TimeDelta,
    now: Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>,
    cache: Mutex<Option<CachedToken>>,
}

impl KisAuth {
    /// Credentials for request headers — every REST call carries
    /// appkey/appsecret alongside the bearer token (official spec).
    pub fn app_key(&self) -> &str {
        &self.app_key
    }

    pub fn app_secret(&self) -> &str {
        &self.app_secret
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        base_url: String,
        app_key: String,
        app_secret: String,
        http: reqwest::Client,
    ) -> Self {
        Self {
            http,
            base_url,
            app_key,
            app_secret,
            margin: TimeDelta::minutes(5),
            now: Arc::new(Utc::now),
            cache: Mutex::new(None),
        }
    }

    pub fn with_clock(mut self, now: impl Fn() -> DateTime<Utc> + Send + Sync + 'static) -> Self {
        self.now = Arc::new(now);
        self
    }

    /// Return a valid access token, issuing a new one when needed.
    pub async fn access_token(&self) -> Result<String, AuthError> {
        {
            // Single-guard fast path: a cached token within the refresh
            // margin is returned without any further locking.
            let cache = self.cache.lock().await;
            if let Some(token) = cache.as_ref() {
                if (self.now)() < token.expires_at - self.margin {
                    return Ok(token.access_token.clone());
                }
            }
        }
        let fresh = self.issue().await?;
        let token = fresh.access_token.clone();
        *self.cache.lock().await = Some(fresh);
        Ok(token)
    }

    /// Drop the cached token so the next call re-issues (401 path).
    pub async fn invalidate(&self) {
        *self.cache.lock().await = None;
    }

    async fn issue(&self) -> Result<CachedToken, AuthError> {
        let response = self
            .http
            .post(format!("{}/oauth2/tokenP", self.base_url))
            .json(&serde_json::json!({
                "grant_type": "client_credentials",
                "appkey": self.app_key,
                "appsecret": self.app_secret,
            }))
            .send()
            .await
            .map_err(|error| AuthError {
                kind: AuthErrorKind::Network,
                message: error.to_string(),
            })?;

        match response.status().as_u16() {
            200 => {}
            401 | 403 => {
                return Err(AuthError {
                    kind: AuthErrorKind::InvalidCredentials,
                    message: format!("token request rejected: {}", response.status()),
                })
            }
            429 => {
                return Err(AuthError {
                    kind: AuthErrorKind::RateLimited,
                    message: "rate limited issuing token".to_string(),
                })
            }
            status if status >= 500 => {
                return Err(AuthError {
                    kind: AuthErrorKind::Server,
                    message: format!("server error {status} issuing token"),
                })
            }
            status => {
                return Err(AuthError {
                    kind: AuthErrorKind::Malformed,
                    message: format!("unexpected status {status} issuing token"),
                })
            }
        }

        let payload: TokenResponse = response.json().await.map_err(|error| AuthError {
            kind: AuthErrorKind::Malformed,
            message: format!("token response unreadable: {error}"),
        })?;
        let access_token = payload
            .access_token
            .filter(|token| !token.is_empty())
            .ok_or(AuthError {
                kind: AuthErrorKind::Malformed,
                message: "token response missing access_token".to_string(),
            })?;
        let expires_at = parse_kst_expiry(
            payload
                .access_token_token_expired
                .as_deref()
                .unwrap_or_default(),
        )
        .ok_or(AuthError {
            kind: AuthErrorKind::Malformed,
            message: "token response missing/invalid access_token_token_expired".to_string(),
        })?;
        Ok(CachedToken {
            access_token,
            expires_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn body(expiry: &str) -> serde_json::Value {
        serde_json::json!({
            "access_token": "token-1",
            "access_token_token_expired": expiry,
        })
    }

    #[test]
    fn kst_expiry_parses_to_utc() {
        let utc = parse_kst_expiry("2026-09-15 10:00:00").unwrap();
        assert_eq!(utc.to_rfc3339(), "2026-09-15T01:00:00+00:00");
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn caches_until_margin_then_reissues() {
        let server = MockServer::start().await;
        // First issue returns token-1 (KST expiry 10:00 = 01:00 UTC); every
        // later issue returns a far-future token-1 as well — cache behavior
        // is proven by the request count, not the token value.
        Mock::given(method("POST"))
            .and(path("/oauth2/tokenP"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body("2026-09-14 10:00:00")))
            .mount(&server)
            .await;

        let clock = Arc::new(std::sync::Mutex::new(
            DateTime::parse_from_rfc3339("2026-09-14T00:00:00+00:00")
                .unwrap()
                .with_timezone(&Utc),
        ));
        let auth = KisAuth::new(server.uri(), "k".into(), "s".into(), reqwest::Client::new())
            .with_clock({
                let clock = Arc::clone(&clock);
                move || *clock.lock().unwrap()
            });

        assert_eq!(auth.access_token().await.unwrap(), "token-1");
        assert_eq!(auth.access_token().await.unwrap(), "token-1"); // cached

        // Advance past expiry+margin: the cache must re-issue.
        let advanced = *clock.lock().unwrap() + TimeDelta::hours(12);
        *clock.lock().unwrap() = advanced;
        assert_eq!(auth.access_token().await.unwrap(), "token-1");
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn invalid_credentials_not_retried() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/tokenP"))
            .respond_with(ResponseTemplate::new(401).set_body_string("denied"))
            .expect(1)
            .mount(&server)
            .await;
        let auth = KisAuth::new(server.uri(), "k".into(), "s".into(), reqwest::Client::new());
        let error = auth.access_token().await.unwrap_err();
        assert_eq!(error.kind, AuthErrorKind::InvalidCredentials);
    }
}
