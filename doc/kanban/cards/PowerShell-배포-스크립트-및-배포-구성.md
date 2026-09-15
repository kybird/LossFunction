---
title: PowerShell 배포 스크립트 및 배포 구성
status: doing
ordinal: 24000
created: 2026-09-14
depends_on: ["저장 계층 SQLite 전환"]
claimed_by: zcode-main
claimed_at: 2026-09-14T20:06-07:00
---

## Goal
<!-- kanban:goal:begin -->
Windows에서 피닉스 VPS로의 빌드-업로드-기동-헬스-중지를 PowerShell 스크립트 한 줄로 수행하게 한다 (Windows ssh.exe 규칙 준수)
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [ ] #1 scripts/deploy/deploy.ps1이 패키징→업로드→이미지 빌드→기동→헬스 확인까지 수행한다
- [ ] #2 status/stop/logs 스크립트가 동작한다
- [ ] #3 compose가 단일 런타임 서비스(SQLite 볼륨, 메모리 상한, 재시작 정책)로 정리된다
<!-- kanban:ac:end -->

## Plan

## Notes

## Handoff

## Result
