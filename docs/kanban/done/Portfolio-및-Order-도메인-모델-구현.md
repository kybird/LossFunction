---
title: Portfolio 및 Order 도메인 모델 구현
status: done
ordinal: 9000
created: 2026-09-14
depends_on: ["시스템 아키텍처 설계 문서화"]
---

## Goal
<!-- kanban:goal:begin -->
종목/포지션/주문/체결을 표현하는 순수 도메인 모델과 불변식을 구현한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 주문 생성/수정/취소에 대한 도메인 규칙이 테스트로 검증된다
- [x] #2 포지션 집계(평균단가, 수량) 계산이 정확함이 테스트로 검증된다
- [x] #3 도메인 모델이 외부 의존(HTTP/DB) 없이 동작한다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-09-14T17:59-07:00 — 순수 도메인 계층: Order(생성 형상 불변식, amend/cancel 규칙, 부분체결 잔량정정 허용), Portfolio(Decimal 평단가 블렌딩, 실현손익, 초과매도 거부, 0수량 행 유지), 도메인 예외 계층. 검증: pytest 75 passed(규칙/집계/순도 가드 테스트 포함), ruff 통과. 커밋 b5095e4
