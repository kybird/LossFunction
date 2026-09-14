# AGENTS.md

<!-- Seeded by `llm-wiki init`. This file is YOURS — edit freely; init never overwrites it. -->

This repo uses **llm-wiki**: `doc/` is the knowledge base, `doc/kanban/` is the work board.
Files are the source of truth. Kanban card files are written ONLY via the CLI — people read them.

## Skill usage — when to use what

| Moment | Skill / command |
|---|---|
| Before starting any task | `llm-wiki search "<keywords>"` — paste the verbatim error string when debugging |
| After fixing a bug / making a decision / discovering something | `wiki-log` skill → a Case in `doc/raw/YYYY-MM-DD.md` with verbatim error + `hash:` grounding |
| When raw logs have accumulated | `wiki-compile` skill → promote to `doc/wiki/` pages, then `llm-wiki compile index` |
| Sanity check of the knowledge base | `wiki-lint` skill or `llm-wiki lint` |
| Planning work (person present) | `kanban-plan` skill → cards via `llm-wiki card new "<title>"` |
| Unattended execution | `work-loop` skill → `llm-wiki pick --claim <name>`, park judgment calls with `handoff` |

## Rules

- Quote error messages **character-for-character** in logs and wiki pages — `llm-wiki lint`
  back-checks every quote and `hash:` against `doc/raw/`.
- Follow `status: deprecated` → `superseded_by` when reading wiki pages.
- The board is a **project** resource, not a branch resource: from any git linked worktree,
  `llm-wiki` reads and writes the **primary worktree's** `doc/kanban/`. `pick`/`done` run in
  a secondary worktree leave uncommitted changes in the primary worktree — intended, commit
  them there. Opt out with `LLM_WIKI_WORKTREE_LOCAL=1`.
- Expand this file with this repo's own conventions. Keep it short — it loads every session.
