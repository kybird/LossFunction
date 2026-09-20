---
title: Watchlist 설정 도입
status: done
ordinal: 6000
created: 2026-09-20
milestone: 모듈 경계 완성 — 시장 데이터 수집 격리
---

## Goal
<!-- kanban:goal:begin -->
종목 유니버스를 WATCHLIST 환경변수(쉼표 구분 종목코드)로 주고 demo 하드코딩을 제거한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 미설정 시 기본 3종목(005930/035420/069500) 동작 유지 — 호환성 테스트
- [x] #2 설정 시 지정 종목으로 스냅샷·데모 동작 테스트
- [x] #3 잘못된 종목코드는 기동 단계에서 거부
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-09-20T15:12-07:00 — Settings.watchlist + WATCHLIST env(쉼표 구분, trim). 미설정 시 기본 3종목 유지, 잘못된/빈 코드는 ConfigError::Invalid로 기동 거부. DemoLoop 하드코딩 제거 — watchlist 파라미터로 유니버스 주입(알려진 3종목은 가격 앵커 유지, 그 외 50000 폴백), main.rs가 settings.watchlist 전달. .env.example에 문서화. 검증: 121 passed(+2: env 오버라이드·거부, 커스텀 watchlist 스냅샷), clippy 0, fmt 클린, check_docs 9 통과
