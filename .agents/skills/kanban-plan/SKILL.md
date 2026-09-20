---
name: kanban-plan
description: Planning loop — turn a plan into board cards the work-loop can consume. Cards are written only via the CLI; decompose with divergence guards, gate what isn't ready, group multi-card plans under a milestone.
skill-version: 4
---
# When to use

- When the user plans work: a feature, a refactor, a research question, "이번 주 할 일 정리".
- When a work session reveals follow-up work — capture it as cards immediately instead of
  letting it evaporate with the session (plan.md 4.1의 파편화가 이것이다).
- NOT during an unattended loop session — that runs `work-loop`, which consumes cards.

# Division of labor (plan.md 2.4)

- 기획(this skill) = **분해**: a plan becomes cards.
- 개발(`work-loop`) = **해소**: pick → done.
- QA = **수렴 강제**: fake dones get reverted; `board report` shows the ratio.
- The person watches progress through `llm-wiki board` / `board report` / `llm-wiki monitor`.
  Plan so that this one screen is enough — 리뷰가 카드 수에 비례하면 무인의 의미가 없다.

# Before creating cards

1. `llm-wiki search "<keywords>"` — the wall may already have an anti-pattern page or an
   abandoned card with the reason recorded. 폐기 사유는 가장 값비싼 정보다.
2. `llm-wiki board` — does a card for this already exist? Extend (`card edit`) or replace
   it (`supersede`); never spawn a near-duplicate. Titles are identifiers (plan.md 3.5):
   no numbers, no versions in titles.

# Milestones — 계획 단위 소속 (plan.md 3.8)

A plan that decomposes into **2+ cards gets one milestone card** — the board's purpose
axis. One-card work needs no milestone (과잉이다).

1. `card new "<계획 제목>" --kind milestone --goal "<대의 한 문장>"` — the Goal is why
   the plan exists; the morning human reads it above the terminal stream.
2. Every member card carries `--milestone "<계획 제목>"` (or `card edit --milestone`
   later — active cards only; terminal membership is history).
3. The milestone is never picked and never closed by hand: `done`/`supersede`/`abandon`
   of the last member auto-completes it (review-parked milestones only report).
   Milestone progress is **derived**, never stored — don't manage its state.

Rules: milestones stay **flat** (a milestone never belongs to another milestone — CLI
rejects it). Gates don't propagate — each member carries its own `--not-before`.
Abandoning a milestone requires its members to be terminal first (CLI enforces, lists them).

# Decomposition rules (divergence guards, plan.md 2.3)

- One card = one context = one commit. If you cannot say what the commit would be, it is
  too big — split it.
- Children must be **strictly smaller** than the parent. When children replace a parent,
  `supersede` it — the parent dissolves, it is not marked done.
- Depth ≤ 3 (plan → subtask → task). Deeper than that means you are writing the work,
  not planning it. (The milestone sits **above** this ladder as grouping, not depth.)
- Do not board what is not ready to start:
  - Time condition → `card new "<t>" --not-before 2026-09-05`. `pick` skips it until then.
  - Observational condition → create it, then `handoff <t> --question "조건: …"`. It waits
    in review; anyone who can show the condition holds runs `resume <t> --note "근거"`.

# Question timing — start-of-work questions are the cheap ones

- In a session where the person is present (planning), ask **before** boarding a card:
  an ambiguous spec answered now costs one question; answered after an unattended loop
  hit the wall, it costs a parked card and a night of latency.
- The person is away, or the answer can arrive asynchronously: board the card and park it
  immediately — `handoff <t> --question "…"` puts it in review with the question; when the
  answer lands, `resume <t> --note "답: …"` sends it back to todo. handoff is "판정을
  사람에게 넘기기", not "끝나고 물어보기" — timing follows whoever can answer.
- An unattended loop session cannot ask before starting (plan.md 2.1 — 승인 대기는
  오프피크 낭비). There, questions are recorded at the wall and answered in bulk by morning.

# Card quality bar

- **Goal**: one sentence a stranger can act on (`--goal`). No goal, no card.
- **AC**: 1–4 items, each verifiable by running or looking at something objective
  (`--ac`, repeat per item). If an AC can only be verified by "읽어보니 되는 것 같다",
  rewrite it — the QA pass reverts evidence-free dones.
- **Dependencies**: `--depends "다른 카드 제목"` — real DAG edges only (cycles are rejected).
- **Notes** (`card edit --note`) are the append-only journal; timestamps are added by the CLI.
- Every write goes through the CLI. A hand-edited card file is out of contract — 사람은 읽기만.

# While the work runs

- Follow-up discovered mid-task → `card new` right away, then continue. The board is the
  memory, not the session.
- Plan changed? `supersede` the stale cards. Direction abandoned? `abandon --reason` —
  the reason is mandatory and flows into `doc/raw/` as anti-pattern material.
- Do not start cards yourself in a planning session — leave them in `todo` for the
  work-loop. 계획과 실행이 같은 세션에 섞이면 파편화가 돌아온다.
