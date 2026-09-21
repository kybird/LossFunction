//! GLM market-analysis integration (wiki: advisory-signals).
//!
//! Calls an OpenAI-compatible chat/completions endpoint, validates the reply
//! against a strict schema — a malformed analysis is rejected, never
//! best-effort parsed — and on any failure returns an explicit `unknown`
//! fallback so trading keeps running without analysis. Models are advisory;
//! their absence must never halt trading.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Regime {
    TrendingUp,
    TrendingDown,
    Range,
    Volatile,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct MarketRegimeAnalysis {
    pub regime: Regime,
    pub confidence: f64,
    pub summary: String,
    #[serde(default)]
    pub risk_notes: Vec<String>,
}

impl MarketRegimeAnalysis {
    fn validate(self) -> Result<Self, GlmError> {
        if !(0.0..=1.0).contains(&self.confidence) {
            return Err(GlmError::schema("confidence outside [0,1]"));
        }
        if self.summary.is_empty() || self.summary.len() > 2000 {
            return Err(GlmError::schema("summary length out of range"));
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum GlmErrorKind {
    Network,
    Server,
    Auth,
    SchemaViolation,
    Empty,
}

#[derive(Debug, thiserror::Error)]
#[error("{kind:?}: {message}")]
pub struct GlmError {
    pub kind: GlmErrorKind,
    pub message: String,
}

impl GlmError {
    fn schema(message: impl Into<String>) -> Self {
        Self {
            kind: GlmErrorKind::SchemaViolation,
            message: message.into(),
        }
    }
}

/// Parse JSON possibly wrapped in markdown code fences.
fn extract_json(text: &str) -> Result<Value, GlmError> {
    let mut stripped = text.trim();
    if stripped.starts_with("```") {
        stripped = stripped.trim_matches('`');
        let without_tag = stripped.strip_prefix("json").unwrap_or(stripped);
        stripped = without_tag.trim();
    }
    serde_json::from_str(stripped)
        .map_err(|error| GlmError::schema(format!("reply is not JSON: {error}")))
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Deserialize)]
struct ChatMessage {
    content: String,
}

pub struct GlmClient {
    http: reqwest::Client,
    api_key: String,
    base_url: String,
    model: String,
}

impl GlmClient {
    /// Read-only accessors for reuse (strategy codegen shares this client).
    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    /// Raw HTTP handle (tests mock it; codegen reuses the connection pool).
    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    pub fn new(api_key: String, base_url: String, model: String, http: reqwest::Client) -> Self {
        Self {
            http,
            api_key,
            base_url,
            model,
        }
    }

    /// One call, one validated analysis or one classified error.
    pub async fn analyze_regime(&self, context: &Value) -> Result<MarketRegimeAnalysis, GlmError> {
        let body = serde_json::json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content":
                    "You are a market analysis service. Reply with ONLY a JSON object with \
                     keys \"regime\" (one of trending_up, trending_down, range, volatile, \
                     unknown), \"confidence\" (0.0-1.0), \"summary\" (string), \"risk_notes\" \
                     (array of strings). No markdown, no extra text."},
                {"role": "user", "content": context.to_string()},
            ],
            "temperature": 0.2,
        });
        let response = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|error| GlmError {
                kind: GlmErrorKind::Network,
                message: error.to_string(),
            })?;

        match response.status().as_u16() {
            200 => {}
            401 | 403 => {
                return Err(GlmError {
                    kind: GlmErrorKind::Auth,
                    message: format!("rejected: {}", response.status()),
                })
            }
            status if status >= 500 => {
                return Err(GlmError {
                    kind: GlmErrorKind::Server,
                    message: format!("server error {status}"),
                })
            }
            status => {
                return Err(GlmError {
                    kind: GlmErrorKind::Empty,
                    message: format!("unexpected status {status}"),
                })
            }
        }

        let payload: ChatResponse = response.json().await.map_err(|_| GlmError {
            kind: GlmErrorKind::Empty,
            message: "unusable response body".to_string(),
        })?;
        let content = payload
            .choices
            .first()
            .ok_or(GlmError {
                kind: GlmErrorKind::Empty,
                message: "no choices in response".to_string(),
            })?
            .message
            .content
            .clone();
        let parsed = extract_json(&content)?;
        let analysis: MarketRegimeAnalysis = serde_json::from_value(parsed)
            .map_err(|error| GlmError::schema(format!("reply violates schema: {error}")))?;
        analysis.validate()
    }
}

/// What the system uses when GLM is unavailable — explicit, not silent.
#[derive(Debug, Clone, PartialEq)]
pub struct AnalysisFallback {
    pub analysis: MarketRegimeAnalysis,
    pub used_fallback: bool,
    pub error_kind: Option<GlmErrorKind>,
}

type ResultCallback = Box<dyn Fn(&Value) + Send + Sync>;

pub struct RegimeAnalysisService {
    client: Option<GlmClient>,
    on_result: Option<ResultCallback>,
}

impl RegimeAnalysisService {
    pub fn new(client: Option<GlmClient>) -> Self {
        Self {
            client,
            on_result: None,
        }
    }

    pub fn with_recording(
        client: Option<GlmClient>,
        on_result: impl Fn(&Value) + Send + Sync + 'static,
    ) -> Self {
        Self {
            client,
            on_result: Some(Box::new(on_result)),
        }
    }

    pub async fn analyze_regime(&self, context: &Value) -> AnalysisFallback {
        let result = match &self.client {
            None => Err(GlmError {
                kind: GlmErrorKind::Empty,
                message: "not configured".to_string(),
            }),
            Some(client) => client.analyze_regime(context).await,
        };
        let fallback = match result {
            Ok(analysis) => AnalysisFallback {
                analysis,
                used_fallback: false,
                error_kind: None,
            },
            Err(error) => AnalysisFallback {
                analysis: MarketRegimeAnalysis {
                    regime: Regime::Unknown,
                    confidence: 0.0,
                    summary: "GLM analysis unavailable; trading continues without it".to_string(),
                    risk_notes: Vec::new(),
                },
                used_fallback: true,
                error_kind: Some(error.kind),
            },
        };
        if let Some(on_result) = &self.on_result {
            on_result(&serde_json::json!({
                "regime": fallback.analysis.regime,
                "confidence": fallback.analysis.confidence,
                "used_fallback": fallback.used_fallback,
                "error_kind": fallback.error_kind,
            }));
        }
        fallback
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const CONTEXT: &str = r#"{"symbols": ["005930"], "recent_returns": [0.01]}"#;

    fn context() -> Value {
        serde_json::from_str(CONTEXT).unwrap()
    }

    fn client(base_url: String) -> GlmClient {
        GlmClient::new(
            "glm-key".into(),
            base_url,
            "glm-4-flash".into(),
            reqwest::Client::new(),
        )
    }

    fn reply(content: &str) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{"message": {"content": content}}],
        }))
    }

    const VALID: &str = r#"{"regime": "trending_up", "confidence": 0.72,
        "summary": "index above MA20 with rising volume",
        "risk_notes": ["concentration in semis"]}"#;

    /// Returns the server alongside its URI — the caller MUST keep the
    /// server alive for the whole request, or the listener can vanish
    /// mid-test and the port get reused (observed as flaky 404s).
    async fn server_with(response: ResponseTemplate) -> (MockServer, String) {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(response)
            .mount(&server)
            .await;
        let uri = server.uri();
        (server, uri)
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn valid_response_parsed() {
        let (_server, base) = server_with(reply(VALID)).await;
        let analysis = client(base).analyze_regime(&context()).await.unwrap();
        assert_eq!(analysis.regime, Regime::TrendingUp);
        assert!((analysis.confidence - 0.72).abs() < 1e-9);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn fenced_json_accepted() {
        let fenced = format!("```json\n{VALID}\n```");
        let (_server, base) = server_with(reply(&fenced)).await;
        let analysis = client(base).analyze_regime(&context()).await.unwrap();
        assert_eq!(analysis.regime, Regime::TrendingUp);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn schema_violation_rejected_without_retry() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(reply(r#"{"regime": "moon_mode", "confidence": 9}"#))
            .expect(1)
            .mount(&server)
            .await;
        let error = client(server.uri())
            .analyze_regime(&context())
            .await
            .unwrap_err();
        assert_eq!(error.kind, GlmErrorKind::SchemaViolation);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn failures_fall_back_without_stopping_the_system() {
        for (response, kind) in [
            (ResponseTemplate::new(503), GlmErrorKind::Server),
            (ResponseTemplate::new(401), GlmErrorKind::Auth),
        ] {
            let (_server, base) = server_with(response).await;
            let service = RegimeAnalysisService::new(Some(client(base)));
            let fallback = service.analyze_regime(&context()).await;
            assert!(fallback.used_fallback);
            assert_eq!(fallback.error_kind, Some(kind));
            assert_eq!(fallback.analysis.regime, Regime::Unknown);
        }
        // Unconfigured client is pure fallback.
        let service = RegimeAnalysisService::new(None);
        let fallback = service.analyze_regime(&context()).await;
        assert!(fallback.used_fallback);
        assert!(fallback.error_kind.is_some());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn results_are_recorded_including_fallbacks() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let recorded = std::sync::Arc::new(AtomicUsize::new(0));
        let counter = {
            let recorded = Arc::clone(&recorded);
            move |_: &Value| {
                recorded.fetch_add(1, Ordering::SeqCst);
            }
        };
        let (_server, base) = server_with(reply(VALID)).await;
        let service = RegimeAnalysisService::with_recording(Some(client(base)), counter);
        let fallback = service.analyze_regime(&context()).await;
        assert!(!fallback.used_fallback);
        assert_eq!(recorded.load(Ordering::SeqCst), 1);
    }
}
