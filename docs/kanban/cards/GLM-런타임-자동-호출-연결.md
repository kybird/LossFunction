---
title: GLM 런타임 자동 호출 연결
status: todo
ordinal: 30000
created: 2026-11-24
milestone: 무소속
---

## Goal
<!-- kanban:goal:begin -->
GLM 클라이언트는 구현 완료돼 있으나 런타임이 자동 호출하지 않는다(GLM_API_KEY 미연결, 명시적 폴백) — 시장 분석 advisory를 런타임에 자동 연결해 폴백 사용 사실이 audit에 남게 한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [ ] #1 GLM_API_KEY 설정 시 런타임이 시장 분석을 자동 호출하고 결과가 advisory 경로로 흐른다
- [ ] #2 미설정·장애 시 명시적 폴백(regime=unknown) 유지 + 폴백 사용이 audit 기록된다
- [ ] #3 기존 전략·주문 경로 변경 없음(clippy·fmt·전체 테스트 통과)
<!-- kanban:ac:end -->

## Plan

## Notes
- 2026-11-24T12:45+09:00 — 지시서 — 무엇을 할 것: 분석 모듈(analysis.rs·glm 클라이언트)과 런타임 주기 사이 미연결 부분을 env 게이트(GLM_API_KEY 유무)로 연결. 무엇을 건드리지 말 것: 주문 경로·리스크 게이트·실시세 WS 경로. 어디서 틀리기 쉬운가: GLM 장애가 거래 루프를 막지 않게 비동기·타임아웃 분리; 폴백 침묵 금지(fail-loud explicit-fallback 규칙)

## Handoff

## Result
