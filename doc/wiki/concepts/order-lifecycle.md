---
status: active
version_context: "lossfunction execution 계층 (2026-09)"
tags: [execution, order, reconciliation]
aliases: [주문 수명주기, order-state-machine, reconciliation, idempotency-key, 중복 주문 차단]
created: 2026-09-14
confidence: 5
---

# 주문 수명주기 (Order Lifecycle)

주문의 상태 전이, 제출, 조정(reconciliation)의 전체 규칙. 핵심 불변식:
**timeout은 실패가 아니라 미확정이다** — broker가 사실의 원천이며, 로컬
상태는 조정 대상이다.

## 상태머신

- 전이 테이블 `TRANSITIONS[from] = {to...}`로 명시. 종결 상태
  (FILLED/CANCELLED/REJECTED)는 빈 집합.
- **UNKNOWN 진입은 PENDING/SUBMITTED에서만**(제출 응답 계류 중). 해소는
  reconciliation 통해서만: SUBMITTED/PARTIALLY_FILLED/FILLED/CANCELLED/
  REJECTED로 복귀.
- 합법 전이마다 audit 이벤트 정확히 1개, 불법 전이는 예외만 + 이벤트 0.

## 게이트웨이 (유일한 제출 경로)

- `client_order_id` 중복은 broker 호출 **전**에 차단 — UNKNOWN 상태의
  재제출도 포함. "재시도 금지"를 규칙이 아니라 기계적 차단으로 강제.
- 에러 이분법: 확정 거부(API_REJECT/INVALID_TOKEN)만 REJECTED. 나머지
  전부(네트워크/timeout/5xx/응답 파손)는 UNKNOWN — MALFORMED도 UNKNOWN인
  이유: 주문은 갔는데 응답만 깨졌을 수 있음.
- broker에 없는 주문 = 도달 못 함 → REJECTED.

## sync vs reconcile (서로 다른 연산)

- **sync**: SUBMITTED/PARTIAL → broker 보고서로 전진(체결 반영).
- **reconcile**: UNKNOWN → broker 사실로 해소.
- 재시작 복구(`recover`): 미결제를 UNKNOWN으로 등록 → 일괄 reconciliation
  → broker 잔고로 포트폴리오 재구성 → 잔존 open 유지. 절차 문서:
  docs/recovery.md.

## First Principles

주문 시스템의 최악 사고는 이중 주문과 상태 왜곡이다. 둘 다 "빠른 재시도"
에서 나온다. 따라서 재시도 경로를 원천 봉쇄하고(중복 차단), 불확실 상태를
명시적 값(UNKNOWN)으로 만들어 조정 절차만이 출구가 되게 한다.

## Related

[[kis-api]], [[trading-core-determinism]], [[fail-loud]]

## Grounding (References)

- doc/raw/2026-09-14.md Case 10 (상태머신, `hash:a2470ac`)
- doc/raw/2026-09-14.md Case 11 (reconciliation, `hash:2d8d4bd`)
- doc/raw/2026-09-14.md Case 14 (런타임 sync/reconcile 분리, `hash:baa55f7`)
