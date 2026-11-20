---
title: Rust 주문 상태머신
status: done
ordinal: 14000
created: 2026-09-15
depends_on: ["Rust 도메인 모델 (Order/Portfolio)"]
---

## Goal
<!-- kanban:goal:begin -->
열거형 상태와 전이 테이블로 상태머신을 구현한다(UNKNOWN은 계류 중 제출에서만 진입, reconciliation으로만 해소)
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 전 (from,to) 쌍 완전 탐사 테스트가 합법 전이 전부 통과/불법 전부 거부를 단정한다
- [x] #2 합법 전이마다 이벤트 1개, 불법 전이는 이벤트 0임이 단정된다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-09-15T00:45-07:00 — 전이 테이블+UNKNOWN 의미론(계류 중 제출에서만 진입, reconciliation으로만 해소)+합법 전이마다 이벤트 1개/불법 0. 전 (from,to) 쌍 완전 탐사 테스트 포함. 검증: cargo test 42 passed, clippy/fmt 클린
