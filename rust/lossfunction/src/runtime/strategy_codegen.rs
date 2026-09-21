//! Natural-language strategy generation — GLM writes Rust, gates decide.
//!
//! Pipeline: description -> GLM (prompt = trait contract + template +
//! file conventions) -> code -> write `strategies_generated.rs` ->
//! `cargo check` gate -> keep (restart activates) or restore the previous
//! file and report the compiler output. A generation that does not compile
//! NEVER lands in the tree.

use crate::analysis::GlmClient;

/// Where the generated file lives (CWD is the rust/ workspace in launcher
/// runs); overridable for tests.
fn target_path() -> std::path::PathBuf {
    if let Ok(override_path) = std::env::var("CODEGEN_TARGET") {
        return override_path.into();
    }
    // Launcher runs from the workspace root (rust/); cargo test runs from
    // the package root (rust/lossfunction). Pick whichever exists.
    for candidate in [
        "lossfunction/src/strategies_generated.rs",
        "src/strategies_generated.rs",
    ] {
        if std::path::Path::new(candidate).exists() {
            return candidate.into();
        }
    }
    "lossfunction/src/strategies_generated.rs".into()
}

/// The system prompt: the full contract a generated file must satisfy.
pub fn system_prompt() -> String {
    r#"You write ONE complete Rust source file for a trading strategy registry slot.
Reply with ONLY the file content — no markdown fences, no commentary.

The file must define EXACTLY this public surface (the crate already compiles it in):

- `pub const KEY: &str` — unique kebab-case key (e.g. "rsi-dip-buy")
- `pub const NAME: &str` — short Korean display name
- `pub const DESCRIPTION: &str` — one-sentence Korean description
- one strategy struct implementing `Strategy`:
  `fn name(&self) -> &str`, `fn version(&self) -> &str` ("1"),
  `fn decide(&self, snapshot: &MarketSnapshot) -> StrategyDecision`
- `pub fn specs() -> Vec<crate::strategy_registry::StrategySpec>` returning ONE spec:
  StrategySpec {{ key: KEY, name: NAME, description: DESCRIPTION,
                  params: "<기본 파라미터 요약>", generated: true }}
- `pub fn build(key: &str, symbols: &[crate::types::Symbol], quantity: i64)
     -> Option<Box<dyn crate::strategy::Strategy>>` matching only KEY

Rules (violations fail the review):
- `decide` is PURE: no randomness, no clock, no I/O. Same snapshot -> same decision.
- Emit OrderIntents only; never touch brokers or storage.
- Past prices come ONLY from `snapshot.history` (completed bar closes, oldest first);
  `snapshot.quotes` are live ticks. Use `crate::history::sma` if a moving average helps.
- If history is insufficient, return empty intents (no forced trade).
- Sell only when `snapshot.positions` shows a holding for the symbol.

Imports you may use: std::collections::BTreeMap, rust_decimal::Decimal,
crate::history::sma, crate::strategy::{{MarketSnapshot, OrderIntent, Strategy, StrategyDecision}},
crate::types::{{OrderSide, OrderType, Symbol}}.

Example shape (SMA touch):

use std::collections::BTreeMap;
use crate::history::sma;
use crate::strategy::{{MarketSnapshot, OrderIntent, Strategy, StrategyDecision}};
use crate::types::{{OrderSide, OrderType, Symbol}};

pub const KEY: &str = "sma-touch";
pub const NAME: &str = "SMA 접촉";
pub const DESCRIPTION: &str = "종가가 N일 이동평균을 상회하면 매수";

pub struct SmaTouchStrategy {{ symbols: Vec<Symbol>, period: usize, quantity: i64 }}
impl SmaTouchStrategy {{ pub fn new(symbols: Vec<Symbol>, period: usize, quantity: i64) -> Self {{ Self {{ symbols, period, quantity }} }} }}
impl Strategy for SmaTouchStrategy {{
    fn name(&self) -> &str {{ KEY }} fn version(&self) -> &str {{ "1" }}
    fn decide(&self, snapshot: &MarketSnapshot) -> StrategyDecision {{
        let mut intents = Vec::new();
        for symbol in &self.symbols {{
            let Some(closes) = snapshot.history.get(symbol) else {{ continue }};
            let Some(avg) = sma(closes, self.period) else {{ continue }};
            let last = closes[closes.len() - 1];
            let holding = snapshot.positions.get(symbol).map(|p| p.quantity).unwrap_or(0);
            if last > avg && holding == 0 {{
                intents.push(OrderIntent {{ symbol: symbol.clone(), side: OrderSide::Buy,
                    order_type: OrderType::Market, quantity: self.quantity, limit_price: None }});
            }}
        }}
        StrategyDecision {{ strategy_name: KEY.into(), strategy_version: "1".into(), intents,
            features: BTreeMap::new(), rationale: String::new() }}
    }}
}}
pub fn specs() -> Vec<crate::strategy_registry::StrategySpec> {{
    vec![crate::strategy_registry::StrategySpec {{ key: KEY, name: NAME, description: DESCRIPTION,
        params: "기간 20", generated: true }}]
}}
pub fn build(key: &str, symbols: &[Symbol], quantity: i64) -> Option<Box<dyn Strategy>> {{
    (key == KEY).then(|| Box::new(SmaTouchStrategy::new(symbols.to_vec(), 20, quantity)) as Box<dyn Strategy>)
}}
"#.to_string()
}

/// Strip markdown fences some models wrap around code.
pub fn strip_fences(reply: &str) -> String {
    let text = reply.trim();
    let without_prefix = text
        .strip_prefix("```rust")
        .or_else(|| text.strip_prefix("```"));
    let body = without_prefix.unwrap_or(text);
    body.strip_suffix("```").unwrap_or(body).trim().to_string()
}

/// Ask GLM for a strategy file implementing `description`.
pub async fn generate_code(client: &GlmClient, description: &str) -> Result<String, String> {
    let body = serde_json::json!({
        "model": client.model(),
        "messages": [
            {"role": "system", "content": system_prompt()},
            {"role": "user", "content": format!("전략 설명: {description}\n\n위 규약에 맞는 파일 하나만 답하라.")},
        ],
        "temperature": 0.1,
    });
    let response = client
        .http()
        .post(format!("{}/chat/completions", client.base_url()))
        .bearer_auth(client.api_key())
        .json(&body)
        .send()
        .await
        .map_err(|error| format!("GLM 호출 실패: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "GLM 응답 {} — 키/모델 확인 필요",
            response.status()
        ));
    }
    let payload: serde_json::Value = response
        .json()
        .await
        .map_err(|error| format!("GLM 응답 파싱 실패: {error}"))?;
    let content = payload["choices"][0]["message"]["content"]
        .as_str()
        .ok_or("GLM 응답에 content 없음")?;
    let code = strip_fences(content);
    if !code.contains("pub fn specs()") || !code.contains("impl Strategy for") {
        return Err("생성 코드가 규약을 충족하지 않음(specs/Strategy 누락)".to_string());
    }
    Ok(code)
}

/// Compile gate: `cargo check` over the crate. Failure restores the
/// previous generated file so the tree always compiles.
pub fn write_and_compile_gate(code: &str) -> Result<(), String> {
    let path = target_path();
    let previous = std::fs::read_to_string(&path).unwrap_or_default();
    std::fs::write(&path, code).map_err(|error| format!("파일 기록 실패: {error}"))?;

    let output = std::process::Command::new("cargo")
        .args(["check", "--quiet"])
        .current_dir(".")
        .output()
        .map_err(|error| {
            let _ = std::fs::write(&path, &previous);
            format!("cargo 실행 실패: {error}")
        })?;

    if output.status.success() {
        Ok(())
    } else {
        let _ = std::fs::write(&path, &previous); // never leave a broken tree
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!(
            "컴파일 실패 — 이전 상태로 복원함\n{}",
            stderr.chars().take(2000).collect::<String>()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_carries_the_contract() {
        let prompt = system_prompt();
        for marker in [
            "pub const KEY",
            "impl Strategy for",
            "pub fn specs()",
            "generated: true",
            "snapshot.history",
        ] {
            assert!(prompt.contains(marker), "prompt missing {marker}");
        }
    }

    #[test]
    fn fences_are_stripped() {
        assert_eq!(strip_fences("```rust\nfn a() {}\n```"), "fn a() {}");
        assert_eq!(strip_fences("fn a() {}"), "fn a() {}");
    }

    /// The gate rejects code that does not compile and restores the real
    /// module file. Serialized: it briefly writes the registered file.
    #[serial_test::serial]
    #[test]
    fn compile_gate_rejects_and_restores() {
        let path = target_path();
        let pristine = std::fs::read_to_string(&path).expect("generated module exists");
        let result = write_and_compile_gate("this is not rust at all {{{");
        assert!(result.is_err(), "broken code must fail the gate");
        assert_eq!(
            std::fs::read_to_string(&path).expect("restored"),
            pristine,
            "previous content restored"
        );
    }
}
