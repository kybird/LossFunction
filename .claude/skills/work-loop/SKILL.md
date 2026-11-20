---
name: work-loop
description: Unattended kanban loop — pick a card, resolve it with objective evidence, park judgment calls and move on. Board is the only task list; converge, don't diverge.
skill-version: 14
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

# Overnight watchdog (쿼터 워치독) — 예약 자동화가 있는 호스트만, 워크스페이스 단위 자가등록

밤샘 무인 실행(호스트 스케줄러 시작 포함)은 쿼터 소진으로 세션이 죽을 수 있다 — 워치독은 쿼터
복구 후 그 세션을 자동 인계한다. 워치독 자동화는 **워크스페이스 규속**이라 이 스킬이 배달되는
각 레포 워크스페이스에서 스스로 등록해야 한다(정본 llm-wiki의 등록은 도그푸드 실증일 뿐):

1. **부트스트랩(이 워크스페이스에 워치독이 없을 때 1회)**: 호스트에 예약 자동화 도구(ZCode
   CronCreate/CronList 등)가 없으면 이 섹션 전체를 건너뛴다. 있으면 자동화 목록에 "쿼터
   워치독"이 이미 있는지 확인하고, 없으면 아래 템플릿의 `<WS>`를 이 워크스페이스 루트
   절대경로로 치환해 등록한다 — 제목 "쿼터 살아나면 밤샘 작업 재개 워치독 (21시~09시, 15분
   주기)", cron `*/15 21-23,0-8 * * *`, recurring. 함께 `<WS>/.zcode/overnight/`에 TASK.md
   스캐폴드(최상단 `status:` 줄 + `## 현재 작업` + `## resume 지침`)와 빈 heartbeat.txt를
   만들고, `.zcode/`가 git에 잡히면 .gitignore에 추가한다.
2. **매 밤샘 시작**: TASK.md의 status를 `running`으로, "현재 작업"과 "resume 지침"(체크포인트 =
   진행 중 카드와 활동 로그, 이어가는 방법)을 채운다.
3. **루프 중**: 카드 1장을 마칠 때마다(한 카드가 15분을 넘으면 중간 단계마다도) heartbeat.txt에
   현재 시각(`YYYY-MM-DD HH:MM:SS`) 한 줄을 append한다.
4. **정지 규칙으로 루프를 끝낼 때** status를 `done`으로 돌려놓는다.

원리: **발사 성공 자체가 쿼터 생존 신호** — 쿼터 소진 중엔 발사가 모델 호출에서 실패(비용 0)하고
15분마다 재발사, 복구 후 첫 성공 발사가 stale heartbeat(20분 이상)를 보고 CLAIM+10분 재확인
뒤 resume 지침대로 인계한다. stale 20분 > 주기 15분이라 살아 있는 세션과 절대 겹치지 않는다.

워치독 발사 프롬프트 템플릿(등록 시 `<WS>` 치환, 경로 구분자는 호스트 규칙을 따름):

```
밤샘 작업 쿼터 워치독 실행이다. 아래 절차를 순서대로 그대로 수행하라.
1. <WS>/.zcode/overnight/TASK.md을 읽는다. 파일이 없거나 status가 'inactive' 또는 'done'이면:
   어떤 작업도 하지 말고 "워치독: 할 일 없음" 한 줄만 출력하고 즉시 종료한다.
2. status가 'running'이면 <WS>/.zcode/overnight/heartbeat.txt를 확인한다(파일이 없으면 비어있는
   것으로 본다). 마지막 줄의 타임스탬프가 현재 시각 기준 20분 이내면 다른 세션이 작업 중인
   것이므로 "워치독: 작업 세션 생존 확인" 한 줄만 출력하고 종료한다.
3. heartbeat가 비어있거나 마지막 기록이 20분 이상 경과했으면: heartbeat.txt에
   "CLAIM <현재 시각 YYYY-MM-DD HH:MM:SS>" 줄을 append하고, Bash로 sleep 600을 실행해 10분
   대기한다(이때 Bash timeout 파라미터를 660000ms로 설정할 것). 대기 후 heartbeat.txt를 다시
   읽어 CLAIM 줄보다 최신 타임스탬프 줄이 새로 생겼으면 "워치독: 세션 인계 불필요" 한 줄 출력하고
   종료한다.
4. 그래도 아무도 활동하지 않으면: TASK.md의 "## resume 지침" 섹션에 적힌 지시에 따라 작업을
   이어서 수행한다. 수행 중에는 5분 이상 공백이 생기지 않게 단계마다 heartbeat.txt에 현재 시각을
   append하고, 완료되면 TASK.md의 status를 'done'으로 수정한 뒤 "워치독: 작업 인계 완료"라고
   보고한다.
참고: 이 실행이 여기까지 도달했다는 것 자체가 모델 쿼터가 살아있다는 뜻이다. 쿼터 소진 상태면
이 실행은 모델 호출 단계에서 실패하고 비용 없이 다음 주기로 넘어간다. TASK.md가 inactive인
평소에는 절대 다른 작업을 시작하지 마라.
```

# Loop graph

```
pick → work → 판정 ─ done / handoff / abandon / supersede / unpick(반납)
  ↑                     │
  └────── next card ←───┘
집을 카드 없음 → review(대기) 큐 점검 → 전부 대기면 질문을 모아 정지 (반스래시)
```

# Rules (all eight are load-bearing)

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
   A **wrongly picked** card (e.g. a `pick --help` accident) is not on the judgment
   list — `llm-wiki unpick <제목> --why "…"` returns it to todo (claim released, WIP
   slot freed, reason recorded); parking it with `handoff` pollutes the review queue.

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
   prints `◉ milestone 완료` closed it for you). That closure is **work finished, not a
   verdict** — do not report a closed milestone as adopted/successful. The route/product
   judgment often arrives days later and is attached without touching state:
   `llm-wiki verdict <제목> --outcome adopted|rejected|superseded --why "…"` (append-only;
   never `reopen` just to record a judgment — reopen is for fake dones only).

5. **Search before work.** `llm-wiki search "<keywords>"` before starting a card. If the
   wall you are about to hit already has an abandoned card or an anti-pattern page, skip
   or supersede — same wall twice in one night is the failure the wiki exists to prevent.

6. **Renew the claim.** A claim expires (board.yml `claim_timeout_minutes`, default 1h).
   On a long card, `llm-wiki card edit <제목> --renew-claim` before the timeout, or
   another loop instance will reclaim the card under you.

7. **Worktrees share one board.** The board is a project resource, not a branch resource:
   from a linked worktree, every `pick`/`done`/`card` writes the **primary worktree's**
   `docs/kanban/`. If this loop runs in a linked worktree, its card changes appear as
   uncommitted changes in the primary worktree — leave them there (the primary's next
   commit picks them up); do not chase them into this worktree's commits, and do not
   commit in the primary from here. `LLM_WIKI_WORKTREE_LOCAL=1` restores per-worktree
   boards.

8. **Interrupt cards go to the unaffiliated backlog.** Follow-ups, user requests, bug cards
   discovered mid-work → `card new "<제목>" --goal "…"` with **no** `--milestone` (2026-09-21
   규약): never glue new work onto a live milestone — that is a scope change the CLI guards
   with `--scope-amend`. The morning ritual triages the backlog — (가) a new milestone,
   (나) an explicit scope change, (다) `abandon`; '나중에' is not an outcome.

# Splitting (supersede) — divergence guard

Supersede only when each child is **strictly smaller** than the parent in context needed.
Guideline while the convergence data is still being collected: card depth ≤ 3, one card =
one context = one commit. `llm-wiki supersede <부모> --by 자식1,자식2` — the parent
dissolves; its history stays in `superseded/`.

# QA pass (개선계획 4.3)

Run at the end of the night, or in a separate morning session:

- Walk recent `docs/kanban/done/` cards newest-first. For each: does `Result` state a real
  verification, and are checked ACs backed by evidence in Notes or the diff?
- Evidence-thin → `llm-wiki reopen <제목> --why "…"` (reverts to doing). A temporary
  increase in card count is the price of a real convergence curve (plan.md 2.4).
- `llm-wiki board report` shows the revert count — reverts are signal, not shame. It also
  shows **review aging** (가장 오래된 대기 일수 · 7일 초과 건수) and **마일스톤 경과일**
  (생성일 기준) — the morning ritual's first item is the review 삼진: every waiting card
  gets answered (`resume --note`) or discarded (`abandon --reason`), 사람이 답한다/버린다.
- A milestone that closed during the night → gather its member `Result`s into a
  human-readable completion record (`DONE.md` 류). The cards already rest in
  `docs/kanban/done/` — promotion is a **summary**, not information movement (2026-09-21 규약).
  Closure means the work finished; if the route/product judgment has since arrived
  (사람 판정·재실험 결과), attach it with `llm-wiki verdict <제목> --outcome … --why "…"`
  — `board report` then counts it under adopted/rejected/superseded, apart from done.
