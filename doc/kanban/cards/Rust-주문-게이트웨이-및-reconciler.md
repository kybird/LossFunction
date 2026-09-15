---
title: Rust 주문 게이트웨이 및 reconciler
status: todo
ordinal: 15000
created: 2026-09-15
depends_on: ["Rust Broker 트레이트 및 MockBroker","Rust 주문 상태머신"]
---

## Goal
<!-- kanban:goal:begin -->
유일 제출 경로 게이트웨이(중복 차단, timeout→UNKNOWN)와 broker 보고 기반 reconciler를 구현한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [ ] #1 timeout 후 같은 id 재제출 차단과 해소(4분기+누락 REJECTED)가 테스트로 단정된다
- [ ] #2 재시작 잔여 일괄 reconciliation이 스크립트 브로커로 검증된다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
