---
title: Order state machine 구현
status: doing
ordinal: 11000
created: 2026-09-14
depends_on: ["Portfolio 및 Order 도메인 모델 구현"]
claimed_by: zcode-main
claimed_at: 2026-09-14T18:05-07:00
---

## Goal
<!-- kanban:goal:begin -->
주문의 PENDING/SUBMITTED/PARTIAL/FILLED/CANCELLED/UNKNOWN 상태 전이를 명시적 state machine으로 구현한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [ ] #1 모든 합법적 상태 전이와 거부되는 전이가 테스트로 검증된다
- [ ] #2 UNKNOWN 상태 진입 경로(timeout 등)가 정의되어 있다
- [ ] #3 모든 전이가 audit 이벤트를 발생시킨다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
