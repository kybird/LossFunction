---
title: 자연어 전략 생성 (GLM)
status: done
ordinal: 7000
created: 2026-09-21
depends_on: ["전략 추가 가이드"]
milestone: 전략 탐색·실험 체계
---

## Goal
<!-- kanban:goal:begin -->
실험실에서 자연어로 전략을 설명하면 GLM이 Rust 코드를 생성하고 컴파일·테스트 게이트를 통과한 것만 레지스트리에 등록한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 POST /control/generate-strategy: 설명→GLM 프롬프트(트레이트+예제 전략+레지스트리 규약 포함)→코드 생성 (프롬프트 구성 테스트)
- [x] #2 생성 코드는 strategies_generated.rs에 기록되고 cargo check+전략 산ity 테스트 통과 시에만 등록 — 실패 시 오류와 함께 거부 (게이트 테스트)
- [x] #3 등록된 생성 전략에 '미검증(생성)' 표시가 전략 목록·백테스트에 나타남 (렌더 테스트)
- [x] #4 실험실 UI: 설명 입력→생성 버튼→코드/컴파일 결과 표시→'재시작 후 활성화' 안내 (테스트)
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-09-21T16:47-07:00 — strategy_codegen: GLM(GlmClient 재사용, open.bigmodel.cn/api/paas/v4·glm-4-flash)에 트레이트 계약+템플릿+파일 규약 프롬프트 → 코드 생성(펜스 제거·규약 검증) → strategies_generated.rs 기록 → cargo check 게이트(실패 시 이전 파일 복원, 깨진 트리 원천 차단). POST /control/generate-strategy(짧은 설명 400, 키 없음 412, audit). 레지스트리 통합(specs/build, generated 플래그 → '미검증·생성' 배지). 실험실 UI: 설명창+생성 버튼+결과(코드/컴파일 출력). 성공은 재시작 후 활성화. 검증: 150 passed(+5: 프롬프트 계약, 펜스, 게이트 거부·복원[실 파일 대상·serial], 400/412, 배지), clippy 0
