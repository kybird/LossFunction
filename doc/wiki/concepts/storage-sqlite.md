---
status: active
version_context: "lossfunction storage 계층 — aiosqlite/WAL (2026-09, PostgreSQL로 시작해 SQLite로 전환)"
tags: [storage, sqlite, data]
aliases: [저장 계층, sqlite-storage, scaled-integer-money]
created: 2026-09-14
confidence: 5
---

# 저장 계층 (SQLite)

단일 파일 SQLite(WAL) 저장소. 전환 사유: 배포 대상(1G RAM VPS)에서
PostgreSQL 상주 메모리(+150~200MB)가 감당 불가했고, 단일 프로세스·단일
작성자 아키텍처에서 SQLite의 약점(동시 쓰기)이 존재하지 않았다. 백업 =
파일 복사.

## First Principles

저장 계층의 책임은 "사실의 지속"뿐이다. 비즈니스 규칙은 도메인에 있다.

## Details

- **화폐는 INTEGER 1e-4원 스케일**(KIS 자체 정밀도). Decimal↔int 경계
  변환(`money_to_int/int_to_money`)이 오차 없음을 round-trip 테스트로 단정.
  반올림은 ROUND_HALF_EVEN.
- 타임스탬프는 **앱이 공급하는 UTC ISO 텍스트** — DB 기본 시계 없음.
  형식이 균일하므로 사전식 정렬=시간순, 재현성 보존.
- 쓰기는 `BEGIN IMMEDIATE` + asyncio.Lock으로 직렬화(설계상 단일 작성자).
  WAL + busy_timeout 5000. JSON은 TEXT로 경계에서 encode/decode.
- **audit은 쓰기 경로의 일부**: 주문/체결/포지션 쓰기가 같은 트랜잭션에서
  audit_log 행을 삽입. 별도 배치가 아니다. quotes는 데이터 수집(비상태
  변경)이므로 제외.
- fills는 자연키(client_order_id, executed_at, quantity, price) UNIQUE로
  멱등 — 재시작/재시도 시 중복 체결 기록 원천 차단.
- 번호 마이그레이션(schema_migrations)은 재실행 no-op.

## Trade-offs

- 동시 다중 읽기-작성 프로세스가 오면 PostgreSQL로 이전해야 한다.
  MLP 대량 분석 워크로드도 마찬가지. 그 시점은 데이터가 증명한다.

## Anti-Pattern

- REAL(float)로 화폐 저장 — 오차 누적.
- DB 기본 시계에 시간 생성 위임 — 형식 불일치로 정렬/재현성 붕괴.
- audit을 나중에 채우는 별도 배치.

## Related

[[boundary-conversions]], [[order-lifecycle]], [[deployment-operations]]

## Grounding (References)

- doc/raw/2026-09-14.md Case 7 (PG 저장 설계 + Windows DLL 사건, `hash:afb6004`)
- doc/raw/2026-09-14.md Case 21 (SQLite 전환, `hash:46f3713`)
