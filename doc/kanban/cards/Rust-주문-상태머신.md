---
title: Rust 주문 상태머신
status: doing
ordinal: 14000
created: 2026-09-15
depends_on: ["Rust 도메인 모델 (Order/Portfolio)"]
claimed_by: zcode-main
claimed_at: 2026-09-15T00:44-07:00
---

## Goal
<!-- kanban:goal:begin -->
열거형 상태와 전이 테이블로 상태머신을 구현한다(UNKNOWN은 계류 중 제출에서만 진입, reconciliation으로만 해소)
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [ ] #1 전 (from,to) 쌍 완전 탐사 테스트가 합법 전이 전부 통과/불법 전부 거부를 단정한다
- [ ] #2 합법 전이마다 이벤트 1개, 불법 전이는 이벤트 0임이 단정된다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
