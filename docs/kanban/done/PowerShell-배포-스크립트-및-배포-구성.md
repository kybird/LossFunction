---
title: PowerShell 배포 스크립트 및 배포 구성
status: done
ordinal: 24000
created: 2026-09-14
depends_on: ["저장 계층 SQLite 전환"]
---

## Goal
<!-- kanban:goal:begin -->
Windows에서 피닉스 VPS로의 빌드-업로드-기동-헬스-중지를 PowerShell 스크립트 한 줄로 수행하게 한다 (Windows ssh.exe 규칙 준수)
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [x] #1 scripts/deploy/deploy.ps1이 패키징→업로드→이미지 빌드→기동→헬스 확인까지 수행한다
- [x] #2 status/stop/logs 스크립트가 동작한다
- [x] #3 compose가 단일 런타임 서비스(SQLite 볼륨, 메모리 상한, 재시작 정책)로 정리된다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
- 2026-09-14T20:18-07:00 — scripts/deploy/ 5종 스크립트(deploy/status/logs/stop/start, Windows ssh.exe 절대경로+~/ 원격경로 규칙). 실증: deploy.ps1 실실행으로 피닉스에 빌드→기동→3초 내 healthy, 호스트 PID SIGKILL로 재시작 정책 실증(restarts=1+헬스 회복), compose 단일 서비스(SQLite 볼륨, mem 150m) 정리. 검증: pytest 178 passed(스크립트 규칙 문서 테스트 포함), ruff 통과. 커밋 4f716c5
