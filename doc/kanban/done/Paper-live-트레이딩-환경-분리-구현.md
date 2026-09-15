---
title: Paper/live 트레이딩 환경 분리 구현
status: done
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
- [x] #1 paper 모드에서 주문이 실제 KIS 주문 endpoint로 전송되지 않음이 테스트로 검증된다
- [x] #2 live 모드 진입이 명시적 설정 확인 절차를 요구한다
- [x] #3 모드가 모든 주문 trace에 기록된다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-09-14T18:05-07:00 — build_broker 팩토리로 모드 분리: paper 기본=메모리 MockBroker(네트워크 0), paper+kis=모의 도메인, live=이중 확인+실전 도메인 재검증. KISBroker가 Broker 완성(submit/cancel(order-rvsecncl 공식 스펙)/positions/quote), 모든 주문 trace에 trading_mode+environment 기록. get_execution_report는 reconciliation 카드로 명시적 이관. 검증: pytest 84 passed, ruff 통과. 커밋 8c36798
