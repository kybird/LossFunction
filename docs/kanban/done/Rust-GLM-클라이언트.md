---
title: Rust GLM 클라이언트
status: done
ordinal: 13000
created: 2026-09-15
depends_on: ["Rust 워크스페이스 부트스트랩"]
---

## Goal
<!-- kanban:goal:begin -->
OpenAI 호환 chat/completions 호출과 엄격 스키마 검증, 장애 시 명시적 폴백을 구현한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 위반 응답 거부(재시도 없음)와 네트워크/서버 실패 폴백이 wiremock 테스트로 검증된다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-09-15T02:14-07:00 — OpenAI 호환 호출+엄격 스키마(위반 즉시 거부·재시도 0 단정, 펜스 허용)+실패 분류+명시적 unknown 폴백(거래 지속)+모든 결과(폴백 포함) 녹화 콜백. 검증: cargo test 86 passed(wiremock), clippy/fmt 클린
