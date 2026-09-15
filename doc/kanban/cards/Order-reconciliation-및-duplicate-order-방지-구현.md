---
title: Order reconciliation 및 duplicate order 방지 구현
status: doing
ordinal: 12000
created: 2026-09-14
depends_on: ["Order state machine 구현","KIS REST 클라이언트 구현"]
claimed_by: zcode-main
claimed_at: 2026-09-14T18:06-07:00
---

## Goal
<!-- kanban:goal:begin -->
timeout/재시작 후 broker 주문 상태 조회로 로컬 주문 상태를 조정하고 중복 주문을 원천 차단한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [ ] #1 주문 timeout 시 reconciliation 완료 전 retry 금지가 테스트로 검증된다
- [ ] #2 재시작 시 미결제 주문(open order) reconciliation이 수행된다
- [ ] #3 중복 주문 시도가 idempotency 검사로 차단됨이 테스트로 검증된다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
