---
title: KIS 기간별시세 MarketDataSource 구현
status: done
ordinal: 3000
created: 2026-09-20
depends_on: ["MarketDataSource 트레이트 정의"]
milestone: 모듈 경계 완성 — 시장 데이터 수집 격리
---

## Goal
<!-- kanban:goal:begin -->
KIS 국내주식 기간별시세(FHKST03010100) 클라이언트를 MarketDataSource로 구현하고 wiremock으로 검증한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 tr_cont 연속 조회 페이지네이션 wiremock 테스트 통과
- [x] #2 스펙 근거를 공식 문서/샘플 확인 후 위키 kis-api 페이지에 기록 (추측 금지)
- [x] #3 실 키 없이 전체 cargo test 녹색
<!-- kanban:ac:end -->

## Plan

## Notes
- 2026-09-20T15:05-07:00 — AC#1 정정: 공식 샘플 확인 결과 이 엔드포인트에는 tr_cont 페이지네이션이 없고 '호출당 최대 100건' 날짜창 분할 방식 — 검증된 메커니즘(창 분할+만페이지 반분 wiremock 테스트)으로 구현함. 근거: doc/raw/2026-09-20.md Case 2

## Handoff

## Result
- 2026-09-20T15:05-07:00 — kis/chart.rs: KisChartSource가 MarketDataSource 구현(FHKST03010100, 수정주가 기본). 100달력일 비중복 창 순회 + 만페이지(100행) 시 명시적 스택으로 창 반분 — 바 DROP 원천 차단. wiremock 4테스트(다중창 병합·만페이지 반분·rt_cd 거부·경계 트림). 스펙 근거를 위키 kis-api 페이지에 기록(공식 샘플+교차검증). 검증: 114 passed, clippy 0, fmt 클린. 실증 대기: rest.rs appkey 빈 헤더 이슈 기록(raw 0920 Case 2)
