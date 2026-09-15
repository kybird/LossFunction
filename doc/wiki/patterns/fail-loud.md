---
status: active
version_context: "lossfunction 오류 처리 관행 (2026-09)"
tags: [error-handling, pattern]
aliases: [status-first-classification, explicit-fallback, fail-boot-on-bad-config]
created: 2026-09-14
confidence: 5
---

# 크게 실패하기 (Fail Loud)

조용한 오답보다 명시적 실패를 택하는 오류 처리 패턴 모음.

## The Rule

1. **status-first-classification**: HTTP 응답은 상태 코드부터 분류하고,
   JSON 파싱은 200일 때만. 비-JSON 에러 페이지에서 파서가 폭발하는 것을
   원천 차단(실측: 503 + text 본문에서 `json.decoder.JSONDecodeError`).
2. **fail-loud-field-mapping**: 외부 API 응답 필드는 문서화된 형상만 사용,
   누락/불일치 시 해당 필드명을 담은 예외로 즉시 실패 — 추측해서 채우지
   않는다.
3. **explicit-fallback**: 실패 시 의미를 유지하는 기본값을 명시적으로
   반환(GLM 장애 → regime=unknown + error_kind 기록)하고 **폴백 사용
   사실까지 audit**. 예외 전파로 거래를 멈추지 않되, 침묵도 없다.
4. **explicit-not-implemented**: 아직 없는 연산은 None/빈 값이 아니라
   전용 예외(PendingReconciliationError 등)로 표시.
5. **fail-boot-on-bad-config**: 치명적 설정 오류(live 미확인 등)는 기동
   거부 — 컨테이너 재시작 정책이 이를 표면화한다. 조용한 폴백 금지.
6. **audit에 실패한 시도는 기록하지 않는다**: 불법 상태 전이는 예외만.
   실패 흔적을 남기면 "시도된 것"과 "실행된 것"의 구분이 흐려진다.

## Why it works

오답은 로그에서 정상처럼 보인다. 실패는 소리를 내야 사람과 재시작 정책이
반응할 수 있다. 특히 거래 시스템에서 "그럴듯한 기본값"은 금전 오류가 된다.

## Trade-offs

- 실패가 잦으면 운영 소음 — 재시도 가능/불가 분류(KISAPIError kind)와
  폴백을 함께 설계한다.

## Anti-Pattern

- except 후 로그만 남기고 None 반환. 응답 파싱을 상태 확인 전에 실행.
- 미구현을 빈 값으로 위장. 폴백 사용을 기록하지 않기.

## Related

[[kis-api]], [[order-lifecycle]], [[deployment-operations]]

## Grounding (References)

- doc/raw/2026-09-14.md Case 5 (파싱 순서 결함, `hash:061eb5a`)
- doc/raw/2026-09-14.md Case 9 (명시적 미구현, `hash:8c36798`)
- doc/raw/2026-09-14.md Case 11 (fail-loud 필드 매핑, `hash:2d8d4bd`)
- doc/raw/2026-09-14.md Case 17 (명시적 폴백, `hash:5c40d6c`)
- doc/raw/2026-09-14.md Case 18 (기동 거부, `hash:9d0a6d3`)
