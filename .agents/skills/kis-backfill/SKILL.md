---
name: kis-backfill
description: Run the KIS daily-bar backfill when the Vaultwarden vault is locked — opens a visible console where the human types only the master password, then polls the status file. Use when daily bars are stale (MAX(ts) old) or before MLP retraining.
skill-version: 1
---
# When to use

- 일봉 백필이 필요한데 금고가 잠겨 있어 에이전트 셸(비대화형)에서
  `scripts/dev/backfill-daily.ps1` 을 못 돌릴 때. 사람이 참석 중이어야 한다(암호 입력).
- 데이터 노후 판정: `SELECT MAX(ts) FROM candles WHERE timeframe='day'` — 직전 영업일보다
  오래됐으면 백필 대상.

# Procedure

1. **런처로 띄우기** — 저장소 루트에서(새 콘솔 창이 뜨고, 그 창에서만 암호가 입력된다):

   ```
   powershell -NoProfile -Command "Start-Process powershell -ArgumentList '-NoExit','-ExecutionPolicy','Bypass','-File','scripts\dev\backfill-daily-agent.ps1' -WorkingDirectory '<저장소 루트 절대경로>'"
   ```

   인자 전달 예: `... backfill-daily-agent.ps1','-Years','10'` (기본 5년, real 도메인).

2. **사람 안내** — "새 콘솔 창에 Vaultwarden 마스터 비밀번호만 입력해 달라"고 요청.
   **암호를 채팅·파일·로그로 받지 말 것** — 마스킹 프롬프트가 있는 그 창이 유일한 경로다.
3. **폴링** — `data/backfill-status.txt` (gitignored):
   - `start=…` 나오면 진행 중(첫 실행은 cargo release 빌드 수 분 포함)
   - `exit=0 …` 이면 성공, `exit=<다른 값>` 이면 실패
4. **완료 검증** — `SELECT symbol, MAX(ts), COUNT(*) FROM candles WHERE timeframe='day' GROUP BY symbol`
   로 갱신 확인. 결과는 wiki-log(verification)으로 기록.

# Rules

- **재실행 폭주 금지**: 500/403 실패 시 프로세스를 다시 띄우지 않는다 — 90초 자동 재시도가
  Rust 백필에 내장돼 있다. 실패는 코드와 함께 보고만.
- **에스컬레이션 상한(2026-11-20 실증)**: 내장 재시도 후에도 500이면 **10분 냉각 후 1회**만
  재실행 허용. 그래도 500이면 **당일 중단하고 익일 재시도** — KIS 단기 제한은 통상 일 단위.
  11-20 실적: 두 번 다 3종목 전멸(500)로 중단, candles 무변경 손실 0 확인.
- 콘솔 출력은 `data/backfill-last-run.log`에 tee된다(실패 원문 보존 — 스크립트는 시크릿을
  출력하지 않으므로 안전).
- **500이 반복되면 본문부터 본다**(raw 2026-11-20 Case 7): KIS는 초당한도(EGW00201)·인증 오류를
  HTTP 500으로 반환 — `backfill-last-run.log`의 msg1이 실제 사유다. 2026-11-20 수정으로 창
  스로틀+EGW00201 재시도가 내장됐다.
- **시크릿 출력 금지**: 상태 파일·로그·채팅에는 시각·종료코드만. 키 값은 백필 프로세스
  트리 안에만 존재한다(구조가 그렇게 설계돼 있다).
- 백필은 조회 API(FHKST03010100)만 쓰며 real 도메인도 읽기 전용이라 안전하다.
- 백필로 데이터가 갱신되면 모델 재학습은 **새 카드**로(진행 중 산출물 무결성 —
  models/meta.json 의 dataset_sha256 이 데이터를 묶고 있다).

# Related

- `scripts/dev/backfill-daily.ps1` 머리글 — 금고 항목 구조("KIS 실전투자"), KIS 규칙 원문
- docs/raw/2026-09-20.md Case 6 — 이 백필 경로의 설계 근거
- docs/raw/2026-09-16.md Case 3 — 개발 머신 비밀번호 프롬프트 이중 경로 패턴
