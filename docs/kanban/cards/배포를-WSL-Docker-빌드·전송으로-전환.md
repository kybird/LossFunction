---
title: 배포를 WSL Docker 빌드·전송으로 전환
status: todo
ordinal: 28000
created: 2026-11-24
milestone: 상시 가동 운영 정비
---

## Goal
<!-- kanban:goal:begin -->
deploy.ps1에 WSL Docker 이미지 빌드+docker save|load 전송 경로를 공식화해 1G 박스 원격 빌드(30분+)를 없앤다 — 11-24 실측(원격 미완료 30분+ vs WSL 수분)
<!-- kanban:goal:end -->

## Acceptance Criteria
<!-- kanban:ac:begin -->
- [ ] #1 deploy.ps1이 WSL 빌드·save|load 전송·compose up·헬스 확인을 비대화형으로 수행한다
- [ ] #2 원격 빌드 경로는 옵션으로 남긴다(WSL 없는 환경 대비)
<!-- kanban:ac:end -->

## Plan

## Notes
- 2026-11-24T12:44+09:00 — 사후 편입 사유: 무인 가동 운영 인프라 주제 묶음 — 11-24 실측(원격 빌드 30분+ vs WSL 수분)으로 우선순위 상승

## Handoff

## Result
