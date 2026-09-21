# 전략 추가 가이드

전략은 `Strategy` 트레이트를 구현하는 순수 함수입니다: 같은 시장 스냅샷이 들어오면
항상 같은 결정을 내놓아야 하고, 주문은 직접 내지 않고 "주문 의도(intent)"만
반환합니다. 실행·리스크·주문 수명주기는 코어가 담당합니다.

## 계약 요약

```rust
pub trait Strategy: Send + Sync {
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    fn decide(&self, snapshot: &MarketSnapshot) -> StrategyDecision;
}
```

`MarketSnapshot`에서 전략이 보는 것:

| 필드 | 의미 | 주의 |
|---|---|---|
| `quotes` | 종목별 최신 체결가(틱) | 진행 중 값 — 아직 완결 아님 |
| `positions` | 현재 포지션 | |
| `history` | 종목별 **완결된 봉 종가** (오래된 순) | 과거 데이터는 이것뿐 — 룩어헤드 원천 차단 |
| `as_of` | 불투명 세션 라벨 | 시각 판단에 사용 금지 |

`StrategyDecision`은 `strategy_name`·`strategy_version`·`intents`(OrderIntent 목록)·
`features`(감사용 피처 요약)를 담습니다. 형태가 잘못된 intent(0수량, 지정가에 가격 없음)
는 런타임이 거부·기록하며 프로세스는 죽지 않습니다.

## 추가 절차 (4단계)

1. `rust/lossfunction/src/strategies.rs` 또는 `strategies2.rs`에 구현
2. 아래 템플릿을 복사해 로직 작성
3. `strategy_registry.rs`에 등록: `registry()`에 `StrategySpec` 추가 + `build()`에
   match arm 추가 (기본 파라미터는 여기서 정의 — 사용자는 파라미터를 만지지 않음)
4. 결정론 테스트 작성 (같은 스냅샷 → 같은 intents)

## 복사 가능 템플릿

```rust
use std::collections::BTreeMap;

use rust_decimal::Decimal;

use crate::history::sma;
use crate::strategy::{MarketSnapshot, OrderIntent, Strategy, StrategyDecision};
use crate::types::{OrderSide, OrderType, Symbol};

/// N일 이동평균 위로 종가가 오르면 매수, 아래로 내려가면 매도.
/// (예시용 단순 구현 — 실제 추가 시 파일 상단 문서 주석을 전략 설명으로 채울 것)
pub struct SmaTouchStrategy {
    symbols: Vec<Symbol>,
    period: usize,
    quantity: i64,
}

impl SmaTouchStrategy {
    pub fn new(symbols: Vec<Symbol>, period: usize, quantity: i64) -> Self {
        Self { symbols, period, quantity }
    }
}

impl Strategy for SmaTouchStrategy {
    fn name(&self) -> &str { "sma-touch" }
    fn version(&self) -> &str { "1" }

    fn decide(&self, snapshot: &MarketSnapshot) -> StrategyDecision {
        let mut intents = Vec::new();
        for symbol in &self.symbols {
            let Some(closes) = snapshot.history.get(symbol) else { continue };
            let Some(avg) = sma(closes, self.period) else { continue }; // 봉이 부족하면 판단 보류
            let last = closes[closes.len() - 1];
            let holding = snapshot.positions.get(symbol).map(|p| p.quantity).unwrap_or(0);
            if last > avg && holding == 0 {
                intents.push(OrderIntent {
                    symbol: symbol.clone(),
                    side: OrderSide::Buy,
                    order_type: OrderType::Market,
                    quantity: self.quantity,
                    limit_price: None,
                });
            }
        }
        StrategyDecision {
            strategy_name: self.name().into(),
            strategy_version: self.version().into(),
            intents,
            features: BTreeMap::new(), // 감사용 피처 요약 (예: {"sma": "81200"})
            rationale: String::new(),
        }
    }
}
```

등록 (`strategy_registry.rs`):

```rust
// registry() 목록에:
StrategySpec {
    key: "sma-touch",
    name: "SMA 접촉",
    description: "종가가 N일 이동평균을 상회하면 매수",
    params: "기간 20",
},
// build() match에:
"sma-touch" => Some(Box::new(SmaTouchStrategy::new(symbols, 20, quantity))),
```

## 규칙 (검증 기준)

- **순수성**: `decide`는 난수·시각·네트워크·전역 상태를 건드리지 않는다.
  내부 상태가 필요하면 생성자 주입으로.
- **intent만**: 브로커·저장소 호출 금지 — 잘못된 intent는 코어가 거부한다.
- **과거는 history로만**: quotes의 과거 축적을 스스로 만들지 않는다.
- **판단 보류는 정상**: 봉이 부족하면 빈 intents를 반환한다 (억지 거래 금지).

## 대안: 자연어 생성

"전략 추가 가이드"의 절차를 GLM이 대신 수행하는 경로가 실험실에 준비 중입니다
(자연어 설명 → 코드 생성 → 컴파일·테스트 게이트 → "미검증(생성)" 표시 등록).
사람이 검증한 전략과 생성 전략은 표시로 구분됩니다.

## 백테스트/시뮬로 검증

등록 즉시 전략은 실험실(`/lab`)의 백테스트와 실데이터 시뮬레이션
(`run-local.ps1 -Mode sim -SimStrategy <키>`)에서 사용 가능합니다.
