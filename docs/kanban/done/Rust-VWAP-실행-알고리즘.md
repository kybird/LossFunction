---
title: Rust VWAP 실행 알고리즘
status: done
ordinal: 30000
created: 2026-09-15
depends_on: ["Rust 주문 게이트웨이 및 reconciler"]
---

## Goal
<!-- kanban:goal:begin -->
대량 주문을 분할해 VWAP 추적 실행하는 실행 알고리즘을 구현한다(리스크 계층 연동)
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 주문 분할 스케줄과 진행률 추적이 테스트로 검증된다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-09-15T12:10-07:00 — VwapSchedule(균등 분할, 포화 기록, Decimal 진행률, 불가분/무효 거부)+vwap() 정확 가격(손계산 단정). 리스크는 기존 게이트웨이 경로 유지. 검증: cargo test 104 passed, clippy/fmt 클린
