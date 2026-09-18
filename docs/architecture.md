# LossFunction 시스템 아키텍처

> 본 문서는 이후 모든 구현 카드의 기준점이다. 모듈 경계나 책임을 변경하는 카드는
> 반드시 이 문서를 먼저 갱신한 뒤 구현한다 (`doc/kanban/` 카드가 이 문서를 의존한다).

## 1. 시스템 개요

LossFunction은 한국투자증권(KIS) Open API 위에서 동작하는 오픈소스 자동투자 시스템이다.

- **대상 시장**: 국내 주식 우선
- **운영 형태**: OCI에서 24/7 무인 운영, Docker 컨테이너
- **언어/플랫폼**: Rust(단일 바이너리, tokio 비동기 런타임) — REST(reqwest) + WebSocket
- **핵심 원칙**: deterministic strategy/risk/order, 전 상태 auditability, restart/recovery

## 2. 컴포넌트 다이어그램

```mermaid
graph TB
    subgraph Runtime["런타임 (오케스트레이터)"]
        ORCH[Trading Runtime<br/>이벤트 루프 + 복구]
    end

    subgraph MarketData["시장 데이터"]
        KIS_WS[KIS WebSocket 클라이언트<br/>실시간 체결/시세]
        KIS_REST_MD[KIS REST 시세 조회]
    end

    subgraph Decision["의사결정"]
        STRAT[Strategy 계층<br/>deterministic decision layer]
        MLP_SVC[MLP 추론<br/>계획: sklearn→ONNX→ort, 미구현]
        GLM_SVC[GLM 분석 서비스<br/>analysis.rs — 스키마 검증+명시적 폴백]
    end

    subgraph Guard["사전 검증"]
        RISK[Risk 계층<br/>한도/kill switch]
    end

    subgraph Execution["실행"]
        BROKER_IFace[Broker Abstraction<br/>인터페이스]
        KIS_BROKER[KIS Broker<br/>live/paper]
        MOCK_BROKER[Mock Broker<br/>테스트/백테스트]
        OSM[Order State Machine]
        RECON[Reconciliation<br/>상태 조정]
    end

    subgraph Persistence["저장"]
        DB[(SQLite<br/>시세/주문/체결/포트폴리오/audit)]
    end

    subgraph Research["연구/검증"]
        BACKTEST[Backtesting 엔진]
        MLP_TRAIN["MLP 학습 파이프라인<br/>reference/ (Python, sklearn)"]
    end

    KIS_WS --> ORCH
    ORCH --> STRAT
    STRAT --> RISK
    RISK --> OSM
    OSM --> BROKER_IFace
    BROKER_IFace --> KIS_BROKER
    BROKER_IFace --> MOCK_BROKER
    KIS_BROKER --> RECON
    RECON --> OSM
    ORCH --> DB
    MLP_SVC --> STRAT
    GLM_SVC --> STRAT
    BACKTEST --> BROKER_IFace
    MLP_TRAIN -.-> MLP_SVC
```

## 3. 데이터 흐름 (정상 경로)

```mermaid
sequenceDiagram
    participant WS as KIS WebSocket
    participant RT as Runtime
    participant ST as Strategy
    participant RK as Risk
    participant OS as Order SM
    participant BR as Broker(live/paper)
    participant DB as SQLite

    WS->>RT: 실시간 시세/체결 이벤트
    RT->>DB: 시세 저장
    RT->>ST: feature snapshot 전달
    ST->>ST: deterministic 신호 생성 (기록됨)
    ST->>RK: 주문 의도 (order intent)
    RK->>RK: 한도/stale data/kill switch 검사
    alt 검사 통과
        RK->>OS: 주문 생성 (PENDING)
        OS->>BR: 주문 제출 (SUBMITTED)
        BR-->>OS: 체결 보고 (PARTIAL/FILLED)
        OS->>DB: 상태 전이 + audit 이벤트
    else 검사 실패
        RK->>DB: 거부 사유 기록 (REJECTED)
    end
```

실패 경로(핵심 불변식):

1. **주문 timeout** → 주문을 FAILED로 표시하지 않는다. `UNKNOWN`으로 전이 후
   reconciliation이 broker측 실제 상태를 확인할 때까지 재시도 금지.
2. **재시작** → 미결제(open) 주문 전수 reconciliation 후에야 새 주문 허용.
3. **중복 주문** → 주문은 idempotency key(client order id)로 식별되고, 동일 key의
   재제출은 차단된다.

## 4. 모듈 경계와 인터페이스 책임

크레이트 루트: `rust/lossfunction/src/` — 의존 방향은 항상 아래 방향(하위 모듈은 상위 모듈을
import하지 않는다).

| 모듈 | 책임 | 하지 않는 것 |
|---|---|---|
| `types.rs`, `history.rs` | 공통 커널 타입(Decimal 가격·정수 주식수·6자리 종목코드), 확정 종가 창(look-ahead 원천 차단) | 네트워크 I/O, DB |
| `domain/` | Order, Position, Portfolio, Fill 순수 모델 + 불변식 | 네트워크 I/O, DB, 외부 라이브러리 의존 |
| `strategy.rs` + `strategies*.rs` | `Strategy` 인터페이스, deterministic decision layer, 신호 기록, 전략 6종(SMA 교차·RSI·돈키안·볼린저·모멘텀 회전·MACD) | 직접 주문 제출 |
| `risk.rs` | 주문 사전 검증: 주문 한도/포지션 상한/총 노출/일일 손실 한도/시세 신선도 검사 + kill switch | 전략 신호 변경 |
| `execution/` | order state machine(`state_machine.rs`), 주문 게이트웨이(`gateway.rs`, 중복 차단), VWAP 실행(`vwap.rs`), reconciliation | 한도 결정 |
| `kis/` | KIS API 클라이언트: auth(tokenP)·REST(tr_cont 페이지네이션)·WebSocket(재연결 시 구독 replay) | 전략 판단, 리스크 판단 |
| `broker/` | `Broker` 트레이트 + Mock 구현. KIS 라이브 구현은 `runtime/assembly.rs`의 `KisBroker`가 담당 | 전략 판단, 리스크 판단 |
| `storage/` | SQLite 스키마/마이그레이션(WAL), 1e-4원 정수 화폐(`money.rs`), 쓰기 트랜잭션 내 audit 로그 | 비즈니스 규칙 |
| `runtime/` | 이벤트 루프, 컴포넌트 조립(`assembly.rs`), axum 서버(`server.rs`/`web.rs` — 헬스·상태 페이지·kill switch 제어), 데모 루프, restart/recovery 절차 | 도메인 규칙 |
| `backtest.rs` | 이벤트 시뮬레이션, 수수료/증권거래세, look-ahead 차단 | 실계좌 접근 |
| `analysis.rs` | GLM 분석 통합: 엄격한 스키마 검증, 장애 시 명시적 폴백(거래 지속) | 실시간 주문 경로 직접 제어 |
| `config.rs` | 설정 로드/검증, 환경(paper/live) 정의, live 이중 확인 가드 | 기본값 하드코딩 |
| `reference/` (저장소 루트) | MLP 학습 파이프라인(Python sklearn — 학습→ONNX 내보내기→Rust `ort` 추론은 계획) | 활성 개발 대상 아님 |

핵심 인터페이스 (구현 카드에서 세부 확정):

- `Broker`: 주문 제출/취소/정정, 잔고 조회, 주문 상태 조회, 시세 구독 — KIS 의존이
  이 경계를 넘지 못한다.
- `Strategy`: feature snapshot → 주문 의도(order intent) 목록. 순수 함수에 가깝게:
  동일 입력에 동일 출력, 내부 상태는 명시적으로 주입.
- `Risk`: (order intent, portfolio 상태, 시장 상태) → 승인/거부 + 사유.
- `OrderStateMachine`: 상태 전이 규칙의 유일한 소유자. 다른 모듈은 주문 상태를
  직접 바꾸지 않는다.

## 5. Paper/Live 환경 분리 원칙

1. **기본값은 paper다.** `TRADING_MODE` 설정이 명시적으로 `live`여야 live broker가
   조립된다.
2. **live 진입은 명시적 확인 절차를 요구한다.** 단순 환경변수 하나로 live 주문이
   나가지 않도록, 설정 조합 검증(예: `LIVE_TRADING_CONFIRMED=true` 별도 확인)이 필요하다.
3. **동일 코드 경로.** paper/live는 `Broker` 구현체 선택으로만 갈린다. 전략/리스크/
   상태머신 코드는 모드를 모른다.
4. **모드는 모든 trace에 기록된다.** 주문, audit 이벤트, 로그에 paper/live 여부가
   남아 사후 추적이 가능하다.
5. **secret은 저장소에 없다.** API key/secret/계좌번호는 환경변수 또는 배포
   시크릿으로만 주입되고, `.gitignore`와 검사로 유출을 차단한다.

## 6. 결정 기록

| 결정 | 근거 |
|---|---|
| 이벤트 중심 단일 프로세스 | 24/7 단일 인스턴스 운영에 충분하고, 분산 시스템 복잡도(네트워크 파티션 시 주문 정합성) 회피 |
| 상태 변경은 전부 audit 이벤트로 기록 | auditability 요구사항, restart/recovery의 원천 데이터 |
| reconciliation 우선 원칙 | timeout ≠ 실패. broker가 사실의 원천(source of truth)이며 로컬 상태는 조정 대상 |
| MLP/GLM은 주문 경로에서 advisory | 비결정적 구성요소가 직접 주문을 내지 못하게 하여 deterministic 원칙 보존 |
| 도메인 모듈은 외부 의존 금지 | 테스트 용이성 + KIS/DB 교체 가능성 |

## 7. 운영 토폴로지 (목표)

```
OCI VM
└── Docker
    └── lossfunction-runtime        (거래 런타임, paper/live)
        └── /data/lossfunction.db   (SQLite 단일 파일 볼륨 — 상태/audit)
```

재시작 시나리오: 컨테이너 재기동 → 저장계층에서 포트폴리오/미결제 주문 복구 →
open order reconciliation → WebSocket 재구독 → 신규 주문 허용.
