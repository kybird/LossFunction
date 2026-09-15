---
title: Rust KIS 인증 및 REST 클라이언트
status: todo
ordinal: 16000
created: 2026-09-15
depends_on: ["Rust Broker 트레이트 및 MockBroker"]
---

## Goal
<!-- kanban:goal:begin -->
토큰(tokenP·KST 만료 파싱·마진 갱신·401 1회 재발급)과 주문/취소/잔고/시세/체결조회 REST를 구현한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [ ] #1 요청 형상(tr_id·대문자 문자열 바디·ODNO 파싱)과 rt_cd!=0 거부 분류가 wiremock으로 검증된다
- [ ] #2 잔고 페이지네이션(tr_cont M/F)이 검증된다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
