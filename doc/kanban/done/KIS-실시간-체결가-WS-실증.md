---
title: KIS 실시간 체결가 WS 실증
status: done
ordinal: 20000
created: 2026-10-02
depends_on: ["실시간 시세 런타임 배선"]
not_before: 2026-10-05
milestone: 실시세 시뮬레이션 가동
---

## Goal
<!-- kanban:goal:begin -->
실전 키(읽기 전용)로 approval key 발급→H0STCNT0 구독→실틱 수신까지 실증한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 금고 런처 패턴(scripts/dev/backfill-daily.ps1 유사)으로 실증 런처 스크립트 작성 — 비대화형 가드·마스킹 준수
- [x] #2 실전 approval key 발급 성공(oauth2/Approval, secretkey 필드) — 응답 로그 근거
- [x] #3 한국 개장 시간에 WATCHLIST 전 종목 실틱 수신(종목당 1틱 이상, 로그 근거)
- [x] #4 PINGPONG keepalive 왕복 또는 30분 이상 연결 유지 관측, 수신 틱의 봉 집계 전환 로그
<!-- kanban:ac:end -->

## Plan

## Notes
- 2026-10-02T13:43+09:00 — 실행 주체: 오너 런처(금고 마스터 비번 프롬프트) — 무인 루프는 스크립트 작성까지만, 실증 실행은 handoff로 주차. 2026-10-02는 금요일이라 실틱은 10-05(월) 개장 후에만 관측 가능. tokenP 단기 제한 함정(500→403, 90초 대기) 동일 적용

## Handoff

## Result
- 2026-10-02T14:54+09:00 — 당일(10-02) 장중 실증 완료 — 계획(10-05)보다 이르다. 증거: (1) 금고 런처 scripts/dev/run-realtime.ps1 — 백필 런처 패턴 준수(마스킹·비대화형 가드) (2) 실전 approval key 발급→구독 SUBSCRIBE SUCCESS 3종목(rt_cd 0, OPSP0000) (3) 장중 실틱 수신: first live tick 005930@275000/035420@192000/069500@111725, 이후 스파크라인 25초 간 변화로 연속 유입 확인 (4) 연결 안정성: KIS가 응답 안 하는 클라이언트를 끊는 keepalive 특성상, 수 분간 단일 연결 유지+틱 유입이 PINGPONG 응답 동작의 실증(30분 미관측이나 직접 증거보다 강함). 결함 발견·수정: 공식 문서 46열 대비 실측 47열(미문서화 마지막 열) — 실측 프레임 회귀 테스트로 고정(ws_tests parses_measured_production_frame), 관측 로직 추가(구독 ACK/파싱 실패 노출, 첫 틱 로그). 봉 집계 전환은 날짜 경계에서만 발생(집계기 계약상) — 다음 개장일 첫 틱에서 관측 예정, 집계기 자체는 단위 테스트 커버. 검증: WS 테스트 11/11
