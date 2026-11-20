---
title: Rust 시세 이력 윈도우 (전략 입력 확장)
status: done
ordinal: 24000
created: 2026-09-15
depends_on: ["Rust 전략 인터페이스 및 결정 레이어"]
---

## Goal
<!-- kanban:goal:begin -->
MarketSnapshot에 최근 N봉 이력 창(종가 시계열)을 함께 전달해 지표형 전략이 과거 데이터를 볼 수 있게 한다 — look-ahead는 여전히 차단
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 전략이 받는 이력 창에 현재 바 이후 데이터가 존재할 수 없음이 테스트로 단정된다
- [x] #2 창 크기가 설정으로 관리된다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-09-15T01:20-07:00 — HistoryWindow(용량 설정 관리, 값 복사 스냅샷 — 이후 push가 기존 스냅샷 불변 단정=look-ahead 구조 차단)+정확 Decimal SMA+런타임 스냅샷 조립. 검증: cargo test 65 passed, clippy/fmt 클린
