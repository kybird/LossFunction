---
title: Rust 기본 전략 3종 (골든크로스·RSI·돈키안)
status: todo
ordinal: 25000
created: 2026-09-15
depends_on: ["Rust 시세 이력 윈도우 (전략 입력 확장)"]
---

## Goal
<!-- kanban:goal:begin -->
가장 널리 쓰이는 기본 알고리즘 3종을 결정론적 전략으로 구현한다: SMA 골든크로스(추추), RSI 과매도 역추세, 돈키안 브레이크아웃
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [ ] #1 각 전략이 동일 입력 2회 동일 출력(결정론)과 대표 시나리오(교차/과매도/신고가) 시그널 테스트를 통과한다
- [ ] #2 세 전략이 백테스팅 엔진에서 비용 반영 손익으로 실행된다
- [ ] #3 파라미터(기간·임계값)가 설정으로 관리된다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
