---
title: Backtesting 엔진 구현
status: doing
ordinal: 16000
created: 2026-09-14
depends_on: ["Strategy 인터페이스 및 deterministic decision layer 구현","Broker abstraction 인터페이스 정의"]
claimed_by: zcode-main
claimed_at: 2026-09-14T18:22-07:00
---

## Goal
<!-- kanban:goal:begin -->
수수료/세금/슬리피지/부분체결/거래시간을 반영하고 look-ahead bias를 차단하는 백테스팅 엔진을 구현한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [ ] #1 거래 비용 반영 손익 계산이 테스트로 검증된다
- [ ] #2 시점 t에서 t 이후 데이터 접근이 차단됨이 테스트로 검증된다
- [ ] #3 동일 데이터 백테스트 결과가 재현 가능함이 검증된다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
