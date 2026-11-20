---
title: Rust 도메인 모델 (Order/Portfolio)
status: done
ordinal: 2000
created: 2026-09-14
depends_on: ["Rust 워크스페이스 부트스트랩"]
---

## Goal
<!-- kanban:goal:begin -->
Order 형상 불변식·amend/cancel 규칙과 Portfolio 평단가/실현손익 집계를 rust_decimal 정확 산술로 구현한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 생성/정정/취소 규칙과 집계(블렌딩·매도·초과매도 거부) 테스트가 통과한다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-09-15T00:23-07:00 — Order(형상 불변식·amend/cancel 가드·RestoredOrder 복구 경로·with_status 단일 변경점)과 Portfolio(Decimal 정확 블렌딩·실현손익·0수량 유지·초과매도 거부)를 Result<_, DomainError>로 구현. 검증: cargo test 15 passed(Python 시맨틱 이식), clippy 0, fmt 통과. 커밋 4a4efb4
