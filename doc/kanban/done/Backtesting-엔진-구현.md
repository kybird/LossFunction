---
title: Backtesting 엔진 구현
status: done
ordinal: 16000
created: 2026-09-14
depends_on: ["Strategy 인터페이스 및 deterministic decision layer 구현","Broker abstraction 인터페이스 정의"]
---

## Goal
<!-- kanban:goal:begin -->
수수료/세금/슬리피지/부분체결/거래시간을 반영하고 look-ahead bias를 차단하는 백테스팅 엔진을 구현한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 거래 비용 반영 손익 계산이 테스트로 검증된다
- [x] #2 시점 t에서 t 이후 데이터 접근이 차단됨이 테스트로 검증된다
- [x] #3 동일 데이터 백테스트 결과가 재현 가능함이 검증된다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-09-14T18:24-07:00 — BacktestEngine: 순차 피드(미래 바 접근 LookaheadError로 차단), 커미션+매도 증권거세 Decimal 정확 반영, 바별 equity 곡선, 라이브와 동일 사건 형상으로 전략 재사용. 검증: pytest 139 passed(수수료/세금 손익 정확성, 미래 접근/미정렬 거부, 스냅샷 감사, 2회 재현 동등), ruff 통과. 커밋 f7c0db4, fd9e97f
