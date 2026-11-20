---
title: Rust 리스크 계층
status: done
ordinal: 5000
created: 2026-09-14
depends_on: ["Rust 도메인 모델 (Order/Portfolio)"]
---

## Goal
<!-- kanban:goal:begin -->
검사 순서(kill switch 최선)와 한도 4종·신선도 검사를 구현한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 한도/신선도/kill switch 우선 테스트가 통과한다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-09-15T00:32-07:00 — 검사 순서(kill switch 최선·견적 없어도 차단)와 한도 4종+신선도(주입 시계) 구현, RiskLimits From<RiskSettings>, kill switch Mutex 상태. 검증: cargo test 28 passed(멀티심볼 gross 포함), clippy/fmt 클린
