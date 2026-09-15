---
title: KIS API 인증 클라이언트 구현
status: doing
ordinal: 5000
created: 2026-09-14
depends_on: ["Broker abstraction 인터페이스 정의","Configuration 및 secrets 환경 분리 구현"]
claimed_by: zcode-main
claimed_at: 2026-09-14T17:06-07:00
---

## Goal
<!-- kanban:goal:begin -->
한국투자증권 Open API의 appkey/appsecret 기반 access token 발급/갱신/만료 처리 클라이언트를 구현한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [ ] #1 토큰 발급/갱신/만료 재발급 흐름이 mock 서버 테스트로 검증된다
- [ ] #2 실제/모의(KIS mock) 도메인이 설정으로 전환된다
- [ ] #3 인증 실패 시 재시도 정책과 에러 분류가 구현되어 있다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
