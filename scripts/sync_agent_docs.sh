#!/usr/bin/env bash
# llm-wiki-template-version: 1 — 이 줄이 살아있으면 llm-wiki init이 갱신하고, 지워져 있으면 사용자 수정본으로 건너뛴다 (plan.md 6.1).
# CLAUDE.md(정본) → 복사본 동기화.
# 복사본 목록(mirrors)에 새 에이전트 파일을 추가하면 자동으로 동기화된다.
# 항상 CLAUDE.md만 편집하고 이 스크립트를 실행할 것.
# (pre-commit 가드가 커밋 시 자동으로 같은 일을 하므로, 보통은 수동 실행 불필요.)
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

canon="CLAUDE.md"
mirrors=("agents.md" "GEMINI.md")

for m in "${mirrors[@]}"; do
  cp -f "$canon" "$m"
  echo "[sync] $canon -> $m"

  if cmp -s "$canon" "$m"; then
    : # OK
  else
    echo "[sync] ERROR: $m 와 $canon 이(가) 불일치. 확인 필요." >&2
    exit 1
  fi
done

echo "[sync] OK: 모든 복사본이 $canon 과(과) 동일합니다."
