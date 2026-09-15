---
title: Order state machine 구현
status: done
ordinal: 11000
created: 2026-09-14
depends_on: ["Portfolio 및 Order 도메인 모델 구현"]
---

## Goal
<!-- kanban:goal:begin -->
주문의 PENDING/SUBMITTED/PARTIAL/FILLED/CANCELLED/UNKNOWN 상태 전이를 명시적 state machine으로 구현한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 모든 합법적 상태 전이와 거부되는 전이가 테스트로 검증된다
- [x] #2 UNKNOWN 상태 진입 경로(timeout 등)가 정의되어 있다
- [x] #3 모든 전이가 audit 이벤트를 발생시킨다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-09-14T18:06-07:00 — 명시적 전이 테이블+종결 상태 불변식, mark_unknown은 계류 중 제출에서만 진입 후 reconciliation으로 해소, 합법 전이마다 audit 이벤트 1개/불법은 이벤트 0. 검증: pytest 96 passed(전 (from,to) 쌍 완전 탐사: 합법 전부 통과/불법 전부 거부), ruff 통과. 커밋 a2470ac
