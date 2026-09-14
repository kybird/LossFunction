---
title: Paper/live 트레이딩 환경 분리 구현
status: todo
ordinal: 10000
created: 2026-09-14
depends_on: ["Broker abstraction 인터페이스 정의","Configuration 및 secrets 환경 분리 구현"]
---

## Goal
<!-- kanban:goal:begin -->
동일한 전략/리스크 코드가 paper broker와 live broker에서 안전하게 전환 실행되도록 환경 분리를 구현한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [ ] #1 paper 모드에서 주문이 실제 KIS 주문 endpoint로 전송되지 않음이 테스트로 검증된다
- [ ] #2 live 모드 진입이 명시적 설정 확인 절차를 요구한다
- [ ] #3 모드가 모든 주문 trace에 기록된다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
