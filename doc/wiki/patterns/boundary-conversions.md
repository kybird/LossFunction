---
status: active
version_context: "lossfunction 경계 변환 관행 (2026-09)"
tags: [data, pattern]
aliases: [경계 변환, boundary-type-conversion, escape-at-boundary]
created: 2026-09-14
confidence: 5
---

# 경계에서의 변환 (Boundary Conversions)

시스템 경계(외부 API ↔ 내부 도메인 ↔ 저장소 ↔ 출력)에서만 타입을 바꾸고,
내부는 하나의 정확한 표현을 유지한다.

## The Rule

- **KST 파싱**: KIS의 타임존 마커 없는 `"YYYY-MM-DD HH:MM:SS"`는 KST로
  해석(`timezone(timedelta(hours=9))` 부여). tz-aware로만 내부 전달.
- **화폐 스케일**: Decimal ↔ INTEGER 1e-4원은 저장소 경계에서만
  (`money_to_int/int_to_money`), round-trip 정확성을 테스트로 단정.
- **JSON 텍스트**: 저장 JSONb/TEXT ↔ dict도 경계에서 decode — 소비자가
  저장 드라이버의 반환 타입(asyncpg text 등)을 몰라야 함.
- **HTML 이스케이프**: DB 값은 렌더링 경계에서 전부 escape(테스트로
  단정). 프레임워크 유무와 무관하게 경계 책임.
- **HTTP 응답 파싱**: 상태 코드 선분류 후 200만 JSON 파싱.

## Why it works

"경계에서만 변환"이 지켜지면 내부 코드는 드라이버/포맷 교체에 면역이 되고,
오차나 이스케이프 누락이 발생할 수 있는 지점이 좁고 명시적이 된다.

## Trade-offs

- 경계 함수가 늘어난다 — 대신 각 변환의 정확성을 한 곳에서 테스트.

## Anti-Pattern

- 도메인 코드에 float 화폐 스케이프. 렌더링 지점마다 개별 이스케이프.
- 소비자가 DB 드라이버 반환 타입을 알아야 하는 구조.

## Related

[[kis-api]], [[storage-sqlite]]

## Grounding (References)

- doc/raw/2026-09-14.md Case 4 (KST 파싱, `hash:4228a47`)
- doc/raw/2026-09-14.md Case 17 (jsonb decode 정규화, `hash:5c40d6c`)
- doc/raw/2026-09-14.md Case 21 (화폐 스케일, `hash:46f3713`)
- doc/raw/2026-09-14.md Case 23 (escape-at-boundary, `hash:22c9209`)
