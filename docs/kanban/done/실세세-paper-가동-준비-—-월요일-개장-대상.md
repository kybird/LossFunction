---
title: 실세세 paper 가동 준비 — 월요일 개장 대상
status: done
ordinal: 28000
created: 2026-11-21
milestone: 무소속
---

## Goal
<!-- kanban:goal:begin -->
피닉스 서버를 QUOTES_SOURCE=kis + REALTIME_STRATEGY=donchian-breakout(기본)으로 전환해 월요일 09:00 개장과 함께 실제 시장 가격·모의 체결(MK_Broker)로 전략 paper 거래를 시작한다. 자격증명은 서버 ~/lossfunction/.env(권한 600)로만 주입 — 화면·저장소 노출 없음. 시세 구독 대상은 보유 15종목
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 서버 헬스에서 broker=MockBroker·trading_mode=paper 확인(실돈 불가 구조 입증) / .env 권한 600·저장소 미추적 / 로그에서 실시세 WS 연결·전략 라벨 확인(틱은 월요일 개장부터) / 데모 루프 중단 전환 확인
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-11-21T15:21+09:00 — 완료 — 월요일 09:00 개장과 함께 실제 시장 가격·모의 체결로 돈키안(기본 20일) paper 시작. 구성: 9전략 빌드 배포 + 자격증명 ssh 파이프 직송(.env 600, 화면·저장소 미노출) + QUOTES_SOURCE=kis·REALTIME_STRATEGY=donchian-breakout·데모 중단 + 서버 일회 컨테이너 백필(15종목 16,451봉 0실패) + 재시작 시딩 16,451봉. 검증: 헬스 broker=MockBroker·paper(실돈 불가 구조 상시 노출), WS 구돊 15종목 전부 SUCCESS(H0STCNT0, 토요일에도 구독 확인 — 틱은 개장부터), 'realtime quotes enabled ... mock fills only' 로그. 함정 기록: 시딩 0봉 갭(리셋 볼륨)은 서버 측 백필로 해결, compose 서비스명은 runtime. 근거: docs/raw/2026-11-21.md Case 5, hash:00a6623
