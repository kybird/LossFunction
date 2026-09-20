---
name: work-loop
description: Unattended kanban loop — pick a card, resolve it with objective evidence, park judgment calls and move on. Board is the only task list; converge, don't diverge.
skill-version: 8
---
# When to use

- A **dedicated unattended session** — user said "루프 돌려" (run the loop) before sleep,
  or a host scheduler started this session at off-peak. The board is the task list; there
  is no competing native todo here.
- Not for ordinary interactive sessions — those should just use `llm-wiki search` and the
  wiki skills.

# Trigger (no daemon — plan.md 4.4)

1. Primary: a human asks, the session runs this skill until the stop rule fires.
2. Secondary: a host-level scheduler (e.g. ZCode 예약) starts the session at midnight.
3. Never build or assume a process supervisor. The loop lives in the session; the board
   lives in files.

# Loop start — bring the monitor up (사람의 실시간 관측)

루프를 시작할 때 `llm-wiki monitor`를 백그라운드로 띄운다 — 밤중에 사람이 들어와도
클레임·진행·마일스톤이 살아 있는 보드를 본다. **멱등이다**: 이 루프의 보드(같은
프로젝트) 모니터가 이미 떠 있으면 exit 0으로 같은 URL을 되뇐다. 다른 프로젝트의
모니터가 기본 포트를 쓰고 있으면 **다음 포트에 자기 보드를 띄운다** — 멀티
프로젝트 밤에도 각자의 보드가 각자의 URL에 뜬다. 백그라운드 실행이 안 되는
환경이면 건너뛴다 — 모니터는 관측이지 의존성이 아니다. 정지 규칙에서도 모니터는
끄지 않는다(아침의 사람이 본다) — 읽기 전용이라 남겨두는 비용이 없다.

# Loop graph

```
pick → work → 판정 ─ done / handoff / abandon / supersede
  ↑                     │
  └────── next card ←───┘
집을 카드 없음 → review(대기) 큐 점검 → 전부 대기면 질문을 모아 정지 (반스래시)
```

# Rules (all seven are load-bearing)

0. **Condition-gated cards.** Some cards must not start yet:
   - `not_before: YYYY-MM-DD` in frontmatter (future date) — `pick` skips them
     automatically; you never see them. Don't try to work around the gate.
   - A card parked in `review` whose Handoff states an **activation condition**
     (e.g. "수렴 데이터 7일 축적 후") — while walking the review queue, you may
     evaluate the condition yourself, but only with objective evidence (`board report`
     numbers, activity.jsonl, repo state). If it holds, `llm-wiki resume <제목>
     --note "조건 충족: <근거>"` sends it back to todo. If not, leave it parked.
   Never resume on vibes. The gate exists so divergence waits its turn.

1. **Judgment = closed list.** Only these four go to `handoff`: ① spec decisions
   ② credentials or outward-facing actions ③ judgment-call merge conflicts
   ④ repeatedly failing tests you cannot diagnose. Everything else — naming, structure,
   implementation details, test approach — you decide yourself. Opening the list wider
   produces chronic park-avoidance in the other direction: an empty done/ pile.

2. **Blocked ≠ stopped.** When a card hits the judgment list, `llm-wiki handoff <제목>
   --question "…"` and immediately pick the next card. Never wait for session approval —
   this session runs unattended (plan.md 2.1: 승인 대기 = 오프피크 낭비). The question is
   a record made at the wall, not a pre-work approval request — 무인 세션에는 시작 전에
   물어볼 상대가 없고, 질문은 밤새 대기 큐에 모여 아침에 일괄 답한다.

3. **AC by objective evidence only.** Check an AC (`card edit --check-ac N`) because you
   ran something that proves it — command output, passing test, diff — never because the
   code "looks right". `done --result` answers exactly two things: 무엇을 바꿨고, 무엇으로
   검증했는가. The QA pass (below) reverts fake dones.

4. **Stop rule.** `pick` returns "No pickable card": check the board's review queue.
   If everything is parked, run `llm-wiki board report` **and** `llm-wiki board video`
   (보고서와 함께 세션 타임랩스 영상을 남긴다 — 아침의 사람이 40초로 밤을 본다),
   leave the questions in one place, and STOP. Do not invent new cards to look
   productive. 밤새 카드가 300장이 되는 것이 이 시스템이 죽는 방식이다 (plan.md 2.3).
   Milestone cards are **never pickable** (they are grouping, not work — plan.md 3.8):
   if the No-pickable detail lists them, that is normal — don't `done` or `abandon` a
   milestone by hand; it auto-completes when its last member terminates (a `done` that
   prints `◉ milestone 완료` closed it for you).

5. **Search before work.** `llm-wiki search "<keywords>"` before starting a card. If the
   wall you are about to hit already has an abandoned card or an anti-pattern page, skip
   or supersede — same wall twice in one night is the failure the wiki exists to prevent.

6. **Renew the claim.** A claim expires (board.yml `claim_timeout_minutes`, default 1h).
   On a long card, `llm-wiki card edit <제목> --renew-claim` before the timeout, or
   another loop instance will reclaim the card under you.

7. **Worktrees share one board.** The board is a project resource, not a branch resource:
   from a linked worktree, every `pick`/`done`/`card` writes the **primary worktree's**
   `doc/kanban/`. If this loop runs in a linked worktree, its card changes appear as
   uncommitted changes in the primary worktree — leave them there (the primary's next
   commit picks them up); do not chase them into this worktree's commits, and do not
   commit in the primary from here. `LLM_WIKI_WORKTREE_LOCAL=1` restores per-worktree
   boards.

# Splitting (supersede) — divergence guard

Supersede only when each child is **strictly smaller** than the parent in context needed.
Guideline while the convergence data is still being collected: card depth ≤ 3, one card =
one context = one commit. `llm-wiki supersede <부모> --by 자식1,자식2` — the parent
dissolves; its history stays in `superseded/`.

# QA pass (개선계획 4.3)

Run at the end of the night, or in a separate morning session:

- Walk recent `doc/kanban/done/` cards newest-first. For each: does `Result` state a real
  verification, and are checked ACs backed by evidence in Notes or the diff?
- Evidence-thin → `llm-wiki reopen <제목> --why "…"` (reverts to doing). A temporary
  increase in card count is the price of a real convergence curve (plan.md 2.4).
- `llm-wiki board report` shows the revert count — reverts are signal, not shame.
