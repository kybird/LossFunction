---
title: Rust ort 추론 어댑터
status: done
ordinal: 25000
created: 2026-11-20
depends_on: ["MLP 학습·ONNX 내보내기"]
milestone: MLP ort 추론 경로
---

## Goal
<!-- kanban:goal:begin -->
ort 크레이트로 ONNX MLP를 로드해 Python 의존 없이 추론하고 MLP-GLM 전략에 advisory 신호로 연결한다
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 고정 입력→고정 출력 회귀 테스트가 Python onnxruntime 결과와 일치한다
- [x] #2 MLP-GLM 전략 advisory 신호가 ort 경로로 계산된다
- [x] #3 clippy·fmt 클린, 전체 테스트 통과
<!-- kanban:ac:end -->

## Plan

## Notes
- 2026-11-20T13:19+09:00 — 지시서 — 무엇을 할 것: ort 크레이트로 ONNX 로드·추론 모듈 추가, 고정 입력 회귀 테스트(Python onnxruntime 산출과 허용오차 비교). 무엇을 건드리지 말 것: 기존 전략 6종·상태머신·저장 계층 변경 금지, advisory 신호는 주문 경로에 개입 금지(리스크 게이트·kill switch 그대로). 어디서 틀리기 쉬운가: ort 실행기 다운로드 방식 있음 — 1G 박스·오프라인 빌드 전제에 맞는 로딩 방식 확인; 결정론(스레드 수·시드) 설정 누락 시 Python 결과와 미세 오차; StandardScaler는 ONNX 그래프에 포함되므로 Rust는 원 피처만 투입

## Handoff

## Result
- 2026-11-20T18:46+09:00 — 완료 — AC 3/3. 신규 rust/lossfunction/src/ml/mod.rs(OnnxMlpDirection: 종목별 세션 로드, 원피처 확률 추론, 로그수익률 피처 산술) + strategy_mlp.rs(MlpAdvisoryStrategy: decide()가 history 종가→ort 확률을 features(mlp_prob_up_*)로 출력, intents 구조상 항상 0개 — 주문 경로 무개입, 결정론 테스트 포함, 레지스트리 미등록은 의도: 주문 전략 목록이 아니므로). 검증: (1) fixture 27행×3심볼 전 행 Python onnxruntime 대비 오차<1e-5·라벨 전 일치, 피처 numpy 대비 <1e-7 (2) advisory 경유 확률=직접 ort 호출 일치·동일 스냅샷 동일 결정 (3) cargo test 169 통과(신규 4), 신규 파일 clippy 0·fmt 적용(backtest* 기존 경고 6건은 범위 밖). ort 2.0.0-rc.15 실측: 프로세스당 Environment 1개(OnceLock 공유 필수), run은 &mut(Mutex 직렬화), extract는 (&Shape,&[f32]) 튜플, 빌드 시 ORT 바이너리 다운로드(오프라인 배포 시 load-dynamic 검토 과제). 근거: docs/raw/2026-11-20.md Case 9, hash:c4ad8cf
