---
title: Broker abstraction 인터페이스 정의
status: done
ordinal: 4000
created: 2026-09-14
depends_on: ["시스템 아키텍처 설계 문서화"]
---

## Goal
<!-- kanban:goal:begin -->
KIS 의존 없이 strategy/risk가 broker를 추상 인터페이스로 사용하는 계층을 정의하고 mock broker를 제공한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 주문/잔고/시세 관련 추상 메서드가 타입 힌트와 함께 정의되어 있다
- [x] #2 테스트용 mock broker가 인터페이스를 구현한다
- [x] #3 인터페이스 준수를 검증하는 테스트가 통과한다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-09-14T17:06-07:00 — async Broker ABC(submit/cancel/report/positions/quote) + frozen record 타입 + client_order_id idempotency key 규격 + MockBroker 구현. 검증: pytest 23 passed(abstract 금지, 준수, idempotency, 평단가, frozen), ruff 통과. 커밋 12dba89
