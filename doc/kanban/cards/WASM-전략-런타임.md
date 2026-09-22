---
title: WASM 전략 런타임
status: review
ordinal: 16000
created: 2026-09-21
milestone: 동적 전략 로딩 (WASM 장기)
---

## Goal
<!-- kanban:goal:begin -->
wasmtime으로 전략 .wasm을 런타임 로드해 Strategy 트레이트 어댑터로 연결 (샌드박스·결정론 유지)
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [ ] #1 wasmtime 의존 및 wasm 모듈 로드→decide 어댑터 (통합 테스트)
- [ ] #2 '미검증·생성' 표시 적용 + 결정론 테스트
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff
- 2026-09-21T21:56-07:00 — QUESTION: 조건 게이트(장기 대기): 피닉스 서버에서 생성 전략 즉시 활성화가 실제로 필요해짐 — 객관적 근거와 함께 resume

## Result
