---
title: Rust 백테스팅 엔진
status: done
ordinal: 8000
created: 2026-09-14
depends_on: ["Rust 전략 인터페이스 및 결정 레이어","Rust Broker 트레이트 및 MockBroker","Rust 리스크 계층"]
---

## Goal
<!-- kanban:goal:begin -->
비용(커미션·매도 세금) 반영, look-ahead 차단(순차 피드), 전략 패리티를 유지한 백테스팅 엔진을 구현한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [ ] #1 비용 정확성·미래 바 접근 거부·재현성 테스트가 통과한다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-09-15T01:29-07:00 — 순차 피드(bar_at 커서 밖 LookaheadError)+완결 봉은 결정 이후 히스토리 진입(체결가 미관측)+Decimal 커미션/매도 세금/바별 equity 곡선+bar 훅으로 목업 가격 주입+리스크 사전검사 재사용. 검증: cargo test 73 passed(비용 정확·미체결 비용 0·재현성·미래 접근 거부), clippy/fmt 클린
