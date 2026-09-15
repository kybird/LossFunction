---
title: Rust 런타임 KIS 백엔드 조립
status: done
ordinal: 24000
created: 2026-09-15
depends_on: ["Rust KIS 인증 및 REST 클라이언트"]
---

## Goal
<!-- kanban:goal:begin -->
진입점이 PAPER_BACKEND=kis/live에서 KisAuth+KisRestClient(+WS)를 조립해 Broker 트레이트로 연결한다(현재는 fail-loud 거부)
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 paper+memory가 아닌 백엔드 설정 시 KIS 클라이언트가 조립되고 설정 오류는 여전히 기동 거부된다
- [x] #2 조립 단위 테스트가 각 백엔드 조합을 검증한다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-09-15T12:15-07:00 — assemble_broker(paper+memory 기본 네트워크 0, paper+kis 모의 도메인, live 이중 확인+real 재검증)+KisBroker 어댑터(Broker 트레이트, 세션 취소 컨텍스트)+조립 단위 테스트 4종. 검증: cargo test 108 passed, clippy/fmt 클린
