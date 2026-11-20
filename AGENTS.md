# AGENTS.md

<!-- Seeded by `llm-wiki init`. This file is YOURS — edit freely; init never overwrites it. -->

This repo uses **llm-wiki**: `docs/` is the knowledge base, `docs/kanban/` is the work board.
Files are the source of truth. Kanban card files are written ONLY via the CLI — people read them.

## Skill usage — when to use what

| Moment | Skill / command |
|---|---|
| Before starting any task | `llm-wiki search "<keywords>"` — paste the verbatim error string when debugging |
| After fixing a bug / making a decision / discovering something | `wiki-log` skill → a Case in `docs/raw/YYYY-MM-DD.md` with verbatim error + `hash:` grounding |
| When raw logs have accumulated | `wiki-compile` skill → promote to `docs/wiki/` pages, then `llm-wiki compile index` |
| Sanity check of the knowledge base | `wiki-lint` skill or `llm-wiki lint` |
| Planning work (person present) | `kanban-plan` skill → cards via `llm-wiki card new "<title>"` |
| Unattended execution | `work-loop` skill → `llm-wiki pick --claim <name>`, park judgment calls with `handoff` |
| Daily bars stale (vault locked) | `kis-backfill` skill → interactive password window + `data/backfill-status.txt` polling |

## Rules

- Quote error messages **character-for-character** in logs and wiki pages — `llm-wiki lint`
  back-checks every quote and `hash:` against `docs/raw/`.
- Follow `status: deprecated` → `superseded_by` when reading wiki pages.
- The board is a **project** resource, not a branch resource: from any git linked worktree,
  `llm-wiki` reads and writes the **primary worktree's** `docs/kanban/`. `pick`/`done` run in
  a secondary worktree leave uncommitted changes in the primary worktree — intended, commit
  them there. Opt out with `LLM_WIKI_WORKTREE_LOCAL=1`.
- Expand this file with this repo's own conventions. Keep it short — it loads every session.

<!-- llm-wiki:section start — 이 블록은 llm-wiki 패키지가 갱신한다(npm update 후 첫 명령).
     마커 사이를 지우면 다음 버전에서 다시 붙는다. 영구 거부는 llm-wiki.config.json
     {"agentsMdSection": false}. 마커 밖 윗·아래 영역은 이 레포의 것이다. -->

This repo uses **llm-wiki**: `docs/` is the knowledge base, `docs/kanban/` is the work board.
Files are the source of truth. Kanban card files are written ONLY via the CLI — people read them.

**레이아웃 계약**: 문서 루트는 `docs/`(복수)가 정본이다. 레거시 `doc/`은 폴백이며 다음 llm-wiki
명령에서 자동 마이그레이션된다. 둘 다 있는 레포는 `docs/` 기준으로 판정한다 — `doc/` 잔재는
구계약의 흔적이지 현재 문서 루트가 아니다.

## 산출물 삼분법 — SSOT (2026-09-21)

살아 있는 할 일 목록은 **보드 하나뿐**이다. 어떤 문서도 '남은 일' 목록을 이중으로 유지하지 않는다.

1. **`AGENTS.md` / `CLAUDE.md` — 상주 규격.** 보드 사용 명세의 집이다. 레포와 함께 산다.
2. **다른 에이전트에게 넘기는 임시 문서 — 협업 요청서.** 넘긴 세션에서만 유효하고 회신되면
   폐기한다. 지속성 있는 일은 카드로만 존재한다. CLADE_TASKS/GLM_TASKS 류의 상주 태스크
   목록은 금지 — 그건 이중 SSOT다.
3. **칸반 보드 (`docs/kanban/`) — 상태 기계이자 남은 일의 유일한 목록.**

## Milestones — 범위 봉인 (scope seal)

- 마일스톤 멤버는 **계획 시점**(`kanban-plan` 분해)의 카드가 전부다 — 그 시점에 결승선이
  확정된다. 마지막 멤버 종결 시 자동 완료되고 손으로 닫지 않는다.
- 진행 중 마일스톤(멤버 중 하나라도 todo/doing/review)에 카드 추가는 **범위 변경**이다:
  `card edit --milestone` 은 `--scope-amend "<사유>"` 가 있을 때만 통과하고, 사유는 카드
  Notes에 '범위 변경'으로 남는다(CLI가 지킨다).
- **인터럽트 카드는 무소속 백로그로** — 작업 중 발견한 follow-up·사용자 요청·버그 카드는
  마일스톤에 바로 붙이지 않고 무소속 todo(`card new`, `--milestone` 없이)로 만든다.

## 아침 정기 의식 — 첫 세션에서

1. **review 삼진이 첫 항목.** review 대기는 마일스톤 완료를 막는 마지막 요소다.
   `board report` 의 노화 지표(가장 오래된 대기 일수, 7일 초과 건수)를 보고 대기 카드마다
   사람이 답한다(`resume --note`) 또는 버린다(`abandon --reason`) — 둘 중 하나를 내린다.
2. **백로그 정리 — '나중에' 금지.** 무소속 todo 카드마다 셋 중 하나로 정리한다:
   (가) 새 마일스톤(`kanban-plan` 이 분해) (나) 명시적 범위 변경(`--scope-amend` + 사유)
   (다) abandon.

## 마일스톤 닫힘 승격과 계획 문서 수명

- 마일스톤이 닫히면 아침 세션(또는 QA pass)이 멤버 Result 를 모아 사람이 읽는 완료
  기록(`DONE.md` 류)으로 승격한다. 카드는 `docs/kanban/done/` 에 이미 남는다 — 승격은
  **요약**이지 정보 이동이 아니다.
- 세션 계획 문서(plan.md 류)는 카드로 소비된 뒤 **병행 갱신 금지**. 종결 시 1회 결과
  회신 후 폐기한다. 실행 중 계획이 바뀌면 문서를 고쳐 맞추지 않고 카드를 `supersede`
  한다 — 실행 중에는 보드가 정본이다.

## 카드 지시서 양식

Note 지시서(`card edit --note`)는 필수 세 항목을 갖춘다:
**무엇을 할 것 / 무엇을 건드리지 말 것 / 어디서 틀리기 쉬운가.**

## Skill usage — when to use what

| Moment | Skill / command |
|---|---|
| Before starting any task | `llm-wiki search "<keywords>"` — paste the verbatim error string when debugging |
| After fixing a bug / making a decision / discovering something | `wiki-log` skill → a Case in `docs/raw/YYYY-MM-DD.md` with verbatim error + `hash:` grounding |
| When raw logs have accumulated | `wiki-compile` skill → promote to `docs/wiki/` pages, then `llm-wiki compile index` |
| Sanity check of the knowledge base | `wiki-lint` skill or `llm-wiki lint` |
| Planning work (person present) | `kanban-plan` skill → cards via `llm-wiki card new "<title>"`. A plan that splits into 2+ cards gets **one milestone card** (`--kind milestone`; members carry `--milestone` at creation — scope is sealed at plan time); milestones are never picked and auto-complete when all members terminate |
| Unattended execution | `work-loop` skill → `llm-wiki pick --claim <name>`, park judgment calls with `handoff` |
| Watching the board (human) | `llm-wiki monitor` — read-only live view (claims, elapsed, gates, terminal pile, milestones) at `http://127.0.0.1:4747` |

## Rules

- Quote error messages **character-for-character** in logs and wiki pages — `llm-wiki lint`
  back-checks every quote and `hash:` against `docs/raw/`.
- Follow `status: deprecated` → `superseded_by` when reading wiki pages.
- The board is a **project** resource, not a branch resource: from any git linked worktree,
  `llm-wiki` reads and writes the **primary worktree's** `docs/kanban/`. `pick`/`done` run in
  a secondary worktree leave uncommitted changes in the primary worktree — intended, commit
  them there. Opt out with `LLM_WIKI_WORKTREE_LOCAL=1`.

<!-- llm-wiki:section end -->
