---
title: 백테스터 candles 데이터 연결
status: done
ordinal: 7000
created: 2026-09-20
depends_on: ["MarketDataSource 트레이트 정의"]
milestone: 모듈 경계 완성 — 시장 데이터 수집 격리
---

## Goal
<!-- kanban:goal:begin -->
백테스터가 candles 테이블에서 종목별 봉을 읽어 수행하도록 연결한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 합성 시드 데이터 기반 백테스트 실행·성과 리포트 생성 테스트 통과
- [x] #2 기존 인메모리 경로 회귀 없음
- [x] #3 봉 부족 종목은 명시적 오류 (조용히 건너뛰지 않음)
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-09-20T15:14-07:00 — backtest.rs에 load_bars_from_candles 추가: candles 테이블→다중 종목 시간순 바 스트림→기존 BacktestEngine.run 그대로. 봉 부족 종목은 BacktestDataError::InsufficientHistory로 심볼명·개수 명시 후 전체 중단(조용한 스킵 금지). 검증: 123 passed(+2: 시드→로더→엔진→리포트 e2e, thin-history 명명 오류), clippy 0, fmt 클린, 기존 인메모리 테스트 무변경 통과
