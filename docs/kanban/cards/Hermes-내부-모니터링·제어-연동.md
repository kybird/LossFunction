---
title: Hermes 내부 모니터링·제어 연동
status: todo
ordinal: 32000
created: 2026-11-28
milestone: 상시 가동 운영 정비
---

## Goal
<!-- kanban:goal:begin -->
같은 피닉스의 hermes-agent가 LossFunction을 관측·제어하게 한다 — docker user network로 컨테이너 간 도달(http://lossfunction:8080, 호스트 노출 추가 0) + 기계판독 JSON 읽기 엔드포인트 + 제어 신뢰 경계 정리(오너 요구 2026-11-28: 이 앱은 터널 외 외부 접근 불가, 내부 에이전트 감시가 유일한 감시층)
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [ ] #1 lossfunction과 hermes가 같은 docker network에 편입되고 hermes 컨테이너에서 http://lossfunction:8080/healthz 호출 성공(호스트 포트 바인딩 변경 없음)
- [ ] #2 GET /api/status JSON이 상태 페이지 데이터를 기계판독으로 제공(모드·브로커·kill switch·WS liveness·포지션·최근 주문/체결)
- [ ] #3 제어 엔드포인트 접근 규칙이 문서화된다(네트워크 신뢰 경계 or 토큰 — 오너 결정 기록)
<!-- kanban:ac:end -->

## Plan

## Notes
- 2026-11-28T12:39+09:00 — 범위 변경: 마일스톤 '상시 가동 운영 정비' 소속 추가 — 오너 요구(2026-11-28): 헤르메스가 이 앱을 모니터링·컨트롤 — 무인 가동의 외부 감시층으로 운영 정비 결승선에 직접 기여

## Handoff

## Result
