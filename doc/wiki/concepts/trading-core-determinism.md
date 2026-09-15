---
status: active
version_context: "lossfunction domain/strategy/ml/analysis 계층 (2026-09)"
tags: [architecture, determinism, strategy]
aliases: [결정론적 거래 코어, deterministic-strategy, advisory-signals, 자문 신호]
created: 2026-09-14
confidence: 5
---

# 결정론적 거래 코어

전략·리스크·주문은 순수 함수 기반: **동일 입력에 동일 결정**, 전 판단 재현
가능. 비결정 요소(MLP/GLM)는 자문(advisory) 입력으로만 들어온다.

## First Principles

자동투자 시스템의 감사 가능성은 "왜 이 주문을 냈는가"에 답할 수 있음에서
나온다. 답하려면 결정이 입력의 함수여야 한다. 따라서 결정론은 규율이 아니라
타입/구조로 강제한다.

## Details

- **Strategy는 MarketSnapshot의 순수 함수** — clock/random/IO 접근 경로
  자체가 없음("구조적으로 결정론적"). 도메인 순도는 테스트로 강제:
  domain 패키지 소스에 httpx/asyncpg/websockets/pydantic_settings 문자열
  금지.
- **StrategyDecision이 신호 도출에 쓰인 feature 원본을 스스로 포함** —
  decision 객체 하나로 사후 재현. DecisionLayer가 모든 결정 녹화.
- **MLP/GLM 출력은 MarketSnapshot.signals에 실리는 자문** — 모델이 직접
  주문하지 않는다. GLM 응답은 엄격 스키마 검증 후에만 통과, 장애 시
  regime=unknown 폴백으로 거래 지속(모델 부재가 시스템을 멈추지 않음).
- 도메인 모델: frozen pydantic + 불변 업데이트(apply_fill이 새 인스턴스
  반환 — 재시작 복구의 재생(replay)이 자연스러움). partially_filled도
  정정 가능(잔량 정정이 실무 표준, "정정 수량 ≥ 이미 체결 수량" 하한).
- **스케일러 통계는 학습 분할만으로 피팅**, 분할은 시간순 + embargo 1행
  (next-bar 라벨이 테스트 기간을 들여다보는 마지막 학습 행 제거).

## Trade-offs

- 결정론이 상태 주입을 번거롭게 만든다 — 대가로 디버깅/백테스트/실전의
  동일성을 산다(백테스트가 라이브와 같은 사건 형상을 사용).

## Anti-Pattern

- 전략이 clock/random/global state 접근. 모델 출력의 직접 주문 경로.
- 녹화를 결정과 분리해 "나중에 채우기". 전체 데이터로 스케일러 피팅.

## Related

[[order-lifecycle]], [[testing-discipline]], [[storage-sqlite]]

## Grounding (References)

- doc/raw/2026-09-14.md Case 8 (도메인 모델, `hash:b5095e4`)
- doc/raw/2026-09-14.md Case 13 (전략 결정론, `hash:b22bda7`)
- doc/raw/2026-09-14.md Case 16 (MLP leakage 방어, `hash:8b31303`)
- doc/raw/2026-09-14.md Case 17 (GLM 스키마/폴백, `hash:5c40d6c`)
- doc/raw/2026-09-14.md Case 15 (백테스트 전략 패리티, `hash:f7c0db4`)
