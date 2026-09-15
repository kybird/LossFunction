---
title: Rust 오케스트레이터 및 데모 루프
status: done
ordinal: 18000
created: 2026-09-15
depends_on: ["Rust 전략 인터페이스 및 결정 레이어","Rust 저장 계층 (SQLite)","Rust 주문 게이트웨이 및 reconciler","Rust 리스크 계층"]
---

## Goal
<!-- kanban:goal:begin -->
견적→결정→리스크→게이트웨이→동기화→포트폴리오 흐름과 시드 랜덤워크 데모 루프를 구현한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 종단 간 데모 틱 테스트(체결→포트폴리오)와 kill switch 차단이 단정된다
- [x] #2 동일 시드 재현성이 단정된다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-09-15T01:08-07:00 — TradingRuntime(종단 흐름+sync/reconcile 분리+recover+broker 잔고 재구성)과 DemoLoop(xorshift64* 결정론 시장, SQLite 전 과정 기록). 검증: cargo test 56 passed(데모 적재·kill 차단·시드 재현), clippy/fmt 클린
