---
title: KIS API 인증 클라이언트 구현
status: done
ordinal: 5000
created: 2026-09-14
depends_on: ["Broker abstraction 인터페이스 정의","Configuration 및 secrets 환경 분리 구현"]
---

## Goal
<!-- kanban:goal:begin -->
한국투자증권 Open API의 appkey/appsecret 기반 access token 발급/갱신/만료 처리 클라이언트를 구현한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 토큰 발급/갱신/만료 재발급 흐름이 mock 서버 테스트로 검증된다
- [x] #2 실제/모의(KIS mock) 도메인이 설정으로 전환된다
- [x] #3 인증 실패 시 재시도 정책과 에러 분류가 구현되어 있다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-09-14T17:10-07:00 — tokenP 기반 토큰 발급/캐시/5분 마진 재발급/싱글플라이트 구현, 에러 6종 분류+일시적 오류만 백오프 재시도. 스펙은 공식 샘플 저장소로 검증(KST 만료 파싱). 검증: pytest 35 passed(MockTransport 기반 발급/캐시/만료/재시도/도메인 전환), ruff 통과. 커밋 4228a47
