---
title: Rust 실행 계층 (상태머신/게이트웨이/reconciliation)
status: superseded
ordinal: 4000
created: 2026-09-14
depends_on: ["Rust 저장 계층 (SQLite)"]
superseded_by: ["Rust 주문 상태머신","Rust 주문 게이트웨이 및 reconciler"]
---

## Goal
<!-- kanban:goal:begin -->
열거형 상태머신(UNKNOWN 의미론 유지), 중복 차단 게이트웨이, reconciler를 구현한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [ ] #1 전 (from,to) 쌍 탐사 테스트와 timeout→UNKNOWN→재제출 차단·해소 테스트가 통과한다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
