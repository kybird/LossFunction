---
title: WASM 전략 런타임 임베딩
status: abandoned
ordinal: 11000
created: 2026-09-21
milestone: 동적 전략 로딩 (WASM)
discard_reason: 사용자 판단: 당분간 불필요 — 개발 머신 재시작 활성화로 충분, WASM 동적 로딩은 요구가 생기면 재등록
---

## Goal
<!-- kanban:goal:begin -->
wasmtime으로 전략 .wasm을 런타임 로드해 Strategy 트레이트 어댑터로 연결한다 (샌드박스·결정론 유지)
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [ ] #1 wasmtime 의존 추가 및 전략 wasm 모듈 로드→decide 호출 어댑터 (통합 테스트)
- [ ] #2 wasm 전략에 '미검증·생성' 표시 동일 적용 + 같은 스냅샷 같은 결정 (결정론 테스트)
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff
- 2026-09-21T21:46-07:00 — QUESTION: 조건 게이트: 피닉스 서버에서 생성 전략의 즉시 활성화가 실제로 필요해짐 (현재는 개발 머신 재시작 활성화로 충분). 조건 성립의 객관적 근거와 함께 resume할 것

## Result
