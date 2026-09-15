---
title: Risk management 계층 구현
status: done
ordinal: 13000
created: 2026-09-14
depends_on: ["Portfolio 및 Order 도메인 모델 구현"]
---

## Goal
<!-- kanban:goal:begin -->
포지션 한도, 집중도, 손실 한도, drawdown, stale data, kill switch를 포함한 주문 사전 검증 리스크 계층을 구현한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 한도 초과 주문이 차단됨이 테스트로 검증된다
- [x] #2 stale market data 상태에서 주문이 차단됨이 테스트로 검증된다
- [x] #3 kill switch 활성화 시 모든 주문 경로가 봉쇄됨이 테스트로 검증된다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-09-14T18:15-07:00 — 사전 검증 RiskManager: 주문 노셔널/종목별 포지션 상한/총 노출(현재가 평가)/일일 실현손실/견적 신선도 검사 + kill switch(최우선, 견적 없어도 차단, 해제 시 재개). 모든 검사는 순수 함수, OrderRejected는 기계 판독 reason. 검증: pytest 116 passed(한도/stale/kill 전 경로), ruff 통과. 커밋 11588c2
