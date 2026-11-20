---
title: Rust MACD 교차 전략
status: done
ordinal: 29000
created: 2026-09-15
depends_on: ["Rust 시세 이력 윈도우 (전략 입력 확장)"]
---

## Goal
<!-- kanban:goal:begin -->
MACD(12,26,9) 교차로 매수/매도하는 추세추종 전략을 구현한다(SMA 교차 보완)
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 골든크로스/데드크로스 시그널과 결정론·백테스트 실행이 검증된다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-09-15T12:08-07:00 — EMA(12,26,9) 정의 손계산 검증+tiny 시리즈 단정, 평탄→점프 시리즈로 마지막 바 교차 단정, 골든크로스 매수. 검증: 99 passed, clippy/fmt 클린
