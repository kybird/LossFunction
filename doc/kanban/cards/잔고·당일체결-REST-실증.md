---
title: 잔고·당일체결 REST 실증
status: todo
ordinal: 22000
created: 2026-10-02
---

## Goal
<!-- kanban:goal:begin -->
실전 키(조회 전용)로 잔고 TTTC8434R·당일체결 TTTC0081R을 실증해 kis-api 위키 pending을 해소한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [ ] #1 실계좌 잔고 조회 성공(보유 0행 응답 형상 포함) — tr_cont 페이지네이션 코드 경로 검증
- [ ] #2 당일체결 조회 — 당일 주문 없음 시에도 rt_cd=0 응답 형상과 필드명 실측 기록
- [ ] #3 kis-api 위키 '커뮤니티 문서 기반, 모의 도메인 실증 pending' 표기를 실측으로 갱신
<!-- kanban:ac:end -->

## Plan

## Notes
- 2026-10-02T13:44+09:00 — 실행 주체: 오너 런처(금고 크레드) — 무인은 조회 클라이언트 실행 스크립트까지만. 주문이 나가지 않는 조회 전용 경로라 실전 도메인 안전. 당일체결 필드 검증은 당일 주문이 있어야 완전하다 — 없으면 형상만 기록

## Handoff

## Result
