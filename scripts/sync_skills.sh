#!/usr/bin/env bash
# llm-wiki-template-version: 1 — 이 줄이 살아있으면 llm-wiki init이 갱신하고, 지워져 있으면 사용자 수정본으로 건너뛴다 (plan.md 6.1).
# .agents/skills/(정본) → .claude/skills/(복사본) 디렉토리 미러링.
# 정본에 없는 스킬/파일은 복사본에서 제거(완전 미러).
# 항상 .agents/skills/만 편집하고 이 스크립트를 실행할 것.
# (pre-commit 가드가 커밋 시 자동으로 같은 일을 하므로, 보통은 수동 실행 불필요.)
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

canon=".agents/skills"
mirror=".claude/skills"

if [ ! -d "$canon" ]; then
  echo "[sync-skills] ERROR: 정본 $canon 이(가) 없습니다." >&2
  exit 1
fi

# 1) 정본 → 복사본 전체 복사 (cp -rf, 기존 파일 덮어쓰기).
mkdir -p "$mirror"
cp -rf "$canon"/. "$mirror"/

# 2) 미러링: 복사본에만 있는 항목(정본에 없는 스킬) 제거.
if [ -d "$mirror" ]; then
  while IFS= read -r item; do
    name=$(basename "$item")
    if [ ! -e "$canon/$name" ]; then
      rm -rf "$mirror/$name"
      echo "[sync-skills] 정본에 없음 → 제거: $mirror/$name"
    fi
  done < <(find "$mirror" -mindepth 1 -maxdepth 1)
fi

# 3) 검증: diff -rq 가 빈 출력이면 완전 미러.
if diff -rq "$canon" "$mirror" >/dev/null 2>&1; then
  echo "[sync-skills] OK: $canon → $mirror 완전 미러."
else
  echo "[sync-skills] ERROR: 동기화 후에도 불일치. diff 확인 필요." >&2
  diff -rq "$canon" "$mirror" >&2 || true
  exit 1
fi
