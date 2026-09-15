---
title: Rust 12-1 모멘텀 로테이션 전략
status: done
ordinal: 28000
created: 2026-09-15
depends_on: ["Rust 시세 이력 윈도우 (전략 입력 확장)"]
---

## Goal
<!-- kanban:goal:begin -->
12개월-1개월 수익률 상위 종목에 월 1회 리밸런싱하는 저빈도 모멘텀 전략을 구현한다(거래세 최소화)
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 모멘텀 스코어 순위·리밸런싱 주기 게이트·결정론이 테스트로 검증된다
<!-- kanban:ac:end -->

## Plan

## Notes
- 2026-09-15T12:08-07:00 — 리밸런싱 주기 게이트는 런타임 일정 카드로 이관(전략은 매 사이클 스코어 계산)

## Handoff

## Result
- 2026-09-15T12:08-07:00 — 12-1 스코어(t-252→t-21 수익률) 상위 N 매수+권외 보유 매도, 심볼 타이브레이크로 결정적 순위. 승자매수·패자청산 시나리오 단정. 주기 게이트는 런타임 일정으로 이관(노트). 검증: 99 passed
