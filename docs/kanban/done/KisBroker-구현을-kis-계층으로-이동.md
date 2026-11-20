---
title: KisBroker 구현을 kis 계층으로 이동
status: done
ordinal: 5000
created: 2026-09-20
milestone: 모듈 경계 완성 — 시장 데이터 수집 격리
---

## Goal
<!-- kanban:goal:begin -->
runtime/assembly.rs 안의 KisBroker 정의와 impl Broker를 kis/로 옮겨 런타임은 조립만 하게 한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 이동 후 runtime/에 'impl Broker for' 0건 (grep 검증)
- [x] #2 동작 무변경 순수 이동 — cargo test 녹색·clippy 0
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-09-20T15:09-07:00 — KisBroker 구조체+impl Broker(주문컨텍스트·dvsn 코드 포함)를 runtime/assembly.rs → kis/broker.rs로 이동. assembly.rs는 AssembledBroker/assemble_broker/build_kis 조립 로직만 남김. 검증: grep 'impl Broker for' runtime/ 0건, cargo test 119 passed(무변경), clippy 0, fmt 클린
