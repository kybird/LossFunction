---
title: Order reconciliation 및 duplicate order 방지 구현
status: done
ordinal: 12000
created: 2026-09-14
depends_on: ["Order state machine 구현","KIS REST 클라이언트 구현"]
---

## Goal
<!-- kanban:goal:begin -->
timeout/재시작 후 broker 주문 상태 조회로 로컬 주문 상태를 조정하고 중복 주문을 원천 차단한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 주문 timeout 시 reconciliation 완료 전 retry 금지가 테스트로 검증된다
- [x] #2 재시작 시 미결제 주문(open order) reconciliation이 수행된다
- [x] #3 중복 주문 시도가 idempotency 검사로 차단됨이 테스트로 검증된다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-09-14T18:12-07:00 — OrderGateway(유일 제출 경로, client_order_id 중복 차단 — UNKNOWN 재제출도 차단), timeout→UNKNOWN 후 OrderSubmitTimeout, 확정 거부만 REJECTED. Reconciler가 broker 보고서로 UNKNOWN 해소+재시작 잔여 일괄 처리, broker 부재 시 REJECTED. KISBroker.get_execution_report를 inquire-daily-ccld로 구현(누락 필드는 크게 실패). 검증: pytest 105 passed, ruff 통과. 커밋 2d8d4bd
