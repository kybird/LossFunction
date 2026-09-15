---
title: KIS REST 클라이언트 구현
status: done
ordinal: 6000
created: 2026-09-14
depends_on: ["KIS API 인증 클라이언트 구현"]
---

## Goal
<!-- kanban:goal:begin -->
주문 제출/취소/정정, 잔고 조회, 현재가 조회 등 KIS REST API 래퍼를 rate limit 처리와 함께 구현한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 주문/잔고/시세 endpoint 호출이 mock 테스트로 검증된다
- [x] #2 API 응답 코드 분류와 에러 매핑이 구현되어 있다
- [x] #3 요청 로그에 audit 가능한 trace가 남는다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-09-14T17:15-07:00 — order-cash/inquire-balance(페이지네이션)/inquire-price 래퍼 구현. 에러 6종 분류(rt_cd!=0 API_REJECT 포함), 일시적 오류 백오프 재시도, 401 시 토큰 1회 재발급 후 재시도. 스펙은 공식 샘플 검증. 검증: pytest 45 passed(요청 형상/거부/재시도/audit 무결성), ruff 통과. 커밋 061eb5a. cancel/체결조회는 reconciliation 카드로
