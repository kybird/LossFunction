---
title: MarketDataSource 트레이트 정의
status: done
ordinal: 2000
created: 2026-09-20
milestone: 모듈 경계 완성 — 시장 데이터 수집 격리
---

## Goal
<!-- kanban:goal:begin -->
과거 봉 조회를 소스 무관 트레이트로 정의하고 candles upsert 저장 함수와 함께 배치한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 합성 소스 → candles upsert → 재적재 시 PK 기반 중복 0 (멱등 테스트 통과)
- [x] #2 marketdata 모듈이 kis 무참조 (grep use crate::kis 0건)
- [x] #3 clippy 0 경고, 전체 테스트 녹색
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-09-20T14:56-07:00 — marketdata 모듈 신설: MarketDataSource 트레이트(async, daily_bars)+Bar(OHLCV)+Timeframe. 저장 계층에 upsert_candles(ON CONFLICT PK 갱신)+candle_count+daily_candles 추가. 검증: cargo test 110 passed(+2: 트레이트 윈도우 필터, 업서트 멱등·수정값 반영), clippy 0, fmt 클린, marketdata의 use crate::kis 0건(grep)
