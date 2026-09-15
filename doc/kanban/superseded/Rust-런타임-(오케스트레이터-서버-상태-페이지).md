---
title: Rust 런타임 (오케스트레이터/서버/상태 페이지)
status: superseded
ordinal: 7000
created: 2026-09-14
depends_on: ["Rust 실행 계층 (상태머신/게이트웨이/reconciliation)","Rust 리스크 계층","Rust KIS 클라이언트 (auth/REST/WS)"]
superseded_by: ["Rust 오케스트레이터 및 데모 루프","Rust axum 서버 (헬스/상태 페이지/제어)"]
---

## Goal
<!-- kanban:goal:begin -->
TradingRuntime(결정→리스크→주문→동기화)과 axum 서버(/healthz, / 상태 페이지, POST /control/kill-switch), 데모 루프를 구현한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [ ] #1 종단 간 데모 틱 테스트와 서브프로세스/통합 서버 테스트(kill 토글+audit)가 통과한다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
