---
title: KIS 토큰 디스크 캐시 — 공식 샘플 패턴 이관
status: todo
ordinal: 26000
created: 2026-11-20
milestone: 상시 가동 운영 정비
---

## Goal
<!-- kanban:goal:begin -->
rust KisAuth가 접근 토큰을 프로세스 내 캐시에서 공식 샘플(kis_auth.py read_token/reAuth)처럼 디스크 캐시로 이관 — token+valid-date 저장, 유효 1일, 재사용 시 tokenP 미호출, 실전/모의 분리 파일. 1일 1회 발급 원칙 준수 강화(11-20 백필 500 재발 계기)
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [ ] #1 유효 토큰 존재 시 tokenP 호출 없이 API 호출 성공(목 서버로 실증) / 토큰 파일은 data/(gitignore) 하위·실전 모의 분리 / 401 403 재발급 경로 기존 동작 유지
<!-- kanban:ac:end -->

## Plan

## Notes
- 2026-11-24T12:44+09:00 — 범위 변경: 마일스톤 '상시 가동 운영 정비' 소속 추가 — 무인 가동 운영 인프라 묶음 명시 확장(2026-11-24 오너 지시: 남은 할일 전량 보드 기록·마일스톤 지정) — 인증 위생은 배포·데이터 수명과 같은 운영 결승선

## Handoff

## Result
