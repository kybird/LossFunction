---
title: 백테스트 설정 UI
status: done
ordinal: 4000
created: 2026-09-21
depends_on: ["백테스트 실행"]
milestone: 전략 탐색·실험 체계
---

## Goal
<!-- kanban:goal:begin -->
BacktestConfig(초기자금·수수료율·거래세율·히스토리 용량)를 프론트엔드 폼에서 수정해 실행에 반영한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 폼 입력값이 실행 설정으로 전달되고 응답에 echo (통합 테스트)
- [x] #2 기본값 표시 + 범위 검증(음수·과대값 거부) (테스트)
- [x] #3 요율이 결과에 반영됨 — 다른 수수료율로 다른 순이익 (테스트)
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-09-21T13:38-07:00 — BacktestCommand.config(초기자금·수수료%·거래세%·히스토리용량) — apply() 범위검증(음수·과대 400+Failed 상태), 폼 입력값 전달(ETF 거래세 0 등), config가 실행에 전달. 검증: 142 passed(+1: 오버라이드 적용·거부), clippy 0, fmt
