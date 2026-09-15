---
title: Rust 볼린저 밴드 역추세 전략
status: done
ordinal: 27000
created: 2026-09-15
depends_on: ["Rust 시세 이력 윈도우 (전략 입력 확장)"]
---

## Goal
<!-- kanban:goal:begin -->
20일 이동평균±2σ 밴드 이탈 시 역방향 진입, 중심 회귀 시 청산하는 볼린저 밴드 전략을 구현한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 밴드 하단 매수/상단 매도 시그널과 결정론·백테스트 실행이 테스트로 검증된다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-09-15T12:08-07:00 — 20SMA±2σ 밴드(경계 f64 sqrt, 나머지 Decimal 정확) 하단 매수/상단 매도(보유 시), 밴드 피처 기록, 결정론 단정. 검증: cargo test 99 passed, clippy/fmt 클린
