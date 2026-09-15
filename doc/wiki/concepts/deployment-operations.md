---
status: active
version_context: "lossfunction runtime/ops + 피닉스 VPS (2026-09)"
tags: [deployment, operations, docker]
aliases: [배포 운영, paper-live-separation, restart-policy, paper-live 분리]
created: 2026-09-14
confidence: 5
---

# 배포와 운영

Docker 단일 컨테이너 + SQLite 볼륨 + PowerShell 배포 스크립트.
배포 대상은 1G RAM VPS(피닉스) — 메모리 상한(150m)과 방화벽 비개방이
기본값이다.

## Paper/Live 분리

- 모드 결정은 `build_broker(settings)` **팩토리가 유일**: paper 기본 =
  메모리 MockBroker(네트워크 0), paper+kis = 모의 도메인, live = 실전
  도메인. 전략/리스크/주문 코드는 `Broker`만 본다.
- live는 설정 로드 단계에서 이중 확인(live_trading_confirmed + real
  도메인) 없으면 거부 — **기동 거부가 정책 시행 메커니즘**. 잘못된
  설정으로 조용히 paper로 폴백하지 않는다.
- 모든 주문/취소 audit trace에 trading_mode + environment 기록.

## 재시작 정책의 실제 의미 (검증됨)

- `restart: unless-stopped`는 **진짜 크래시에만** 작동한다. 검증 방법:
  호스트에서 컨테이너 프로세스 PID에 SIGKILL(`docker inspect -f
  {{.State.Pid}}` + sudo kill -9) → restarts=1 + 헬스 회복 확인.
- `docker stop`/`docker kill`은 의도적 정지로 취급되어 재시작 안 함.
  컨테이너 내부 `kill -9 1`은 PID1 신호 보호로 죽지도 않는다.
  "kill했는데 안 살아난다"는 정책 오류가 아니라 측정 방법 오류.

## 운영 요소

- 헬스: `GET /healthz`(JSON, 모드/브로커/업타임) + `GET /`(HTML 상태
  페이지, SQLite에서 렌더). 컨테이너 HEALTHCHECK 30s.
- 알림: 콘솔 상시 + 웹훅 옵트인(ALERT_WEBHOOK_URL). 이벤트별 중복 억제
  창(기본 30s). **채널 실패는 격리** — 알림 인프라가 거래 경로를
  무너뜨리지 않음.
- 배포: `scripts/deploy/deploy.ps1`(패키징→업로드→원격 빌드→기동→헬스).
  Windows ssh.exe 절대경로 + 원격 ~/ 상대경로(피닉스 규칙).
- 복구: 재시작 후 docs/recovery.md 절차(미결제 reconciliation → 잔고
  재구성 → 재구독 → 신규 주문).

## Trade-offs

- 금고 서버(Vaultwarden)와 공유 박스: 메모리 상한과 loopback 포트로
  격리. 풀스택 확장 시 별도 인스턴스 필요.

## Related

[[windows-polluted-service-env]], [[order-lifecycle]], [[fail-loud]]

## Grounding (References)

- doc/raw/2026-09-14.md Case 2 (설정 이중 확인, `hash:8345787`)
- doc/raw/2026-09-14.md Case 9 (paper/live 팩토리, `hash:8c36798`)
- doc/raw/2026-09-14.md Case 18 (배포/헬스, `hash:9d0a6d3`)
- doc/raw/2026-09-14.md Case 19 (알림, `hash:4a2bd72`)
- doc/raw/2026-09-14.md Case 22 (배포 스크립트+재시작 실증, `hash:4f716c5`)
- doc/raw/2026-09-14.md Case 23 (상태 페이지, `hash:22c9209`)
