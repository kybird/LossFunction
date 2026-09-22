---
title: 파라미터 walk-forward 탐색
status: done
ordinal: 14000
created: 2026-09-21
milestone: 전략 최적화
---

## Goal
<!-- kanban:goal:begin -->
전략별 파라미터 그리드를 훈련/검증 구간으로 나눠 탐색하고 과적합을 걸러낸 결과를 보고한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 탐색을 통과한 파라미터는 해당 전략의 '제안 기본값'으로 전략 목록에 표시·제공된다
- [x] #2 훈련 구간 최적 → 미사용 검증 구간 성과로 랭킹, 상위 후보와 기본값 대비 표 (계산 테스트)
- [x] #3 실험실 UI: 전략 선택→탐색 실행→결과(기본 대비 개선율), 백그라운드 상태머신 (테스트)
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-09-21T23:10-07:00 — backtest_optimize.rs: 전략별 파라미터 격자(≤16조합, 기본 포함) → 훈련 70% 랭킹 → 상위+기본만 검증 30% 재시험 → 검증 수익률 최종 랭킹(과적합 통제). 최적 조합은 strategy_suggestions(migration v4)에 영속 → 전략 목록 '제안 7/40 (+2.1%p)' 배지(제안 기본값 제공). 실험실 탐색 컨트롤+상태 라인, POST /control/optimize(404/409, 오프라인 — candles만). 검증: 159 passed(+5: 격자 경계·랭킹·결정론, 최소 이력, e2e+제안 영속, 배지 렌더), clippy 0, fmt
