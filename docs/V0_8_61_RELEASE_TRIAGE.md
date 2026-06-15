# v0.8.61 Release Triage — Goal Mode + Durable Worker Orchestration + Permissions

> Status: **in progress** (prepared by an agent handoff session). Nothing here has
> been tagged, published, merged, closed, or commented on GitHub. All PR/issue
> actions below are **recommendations for Hunter**, not executed actions.
>
> Branch under release: `codex/v0.8.61` (local only — the remote has only `main`,
> both at `ae101a123`). The working tree carries an uncommitted version bump to
> 0.8.61; it has been left untouched. Chunk work was done in an isolated worktree
> off `ae101a123`.

## 1. Release thesis

v0.8.61 is the **"Goal Mode + durable worker orchestration + permissions"** release.
The honest dependency order (which differs from a naive "Goal Mode first"):

1. **Fix the worker substrate** — the TUI hard-freeze on sub-agent fanout is the
   launch blocker (#3216, #2211). Nothing else ships on a TUI that freezes at six
   sub-agents.
2. **Stop the prompt from driving the freeze** — the base prompt actively tells the
   model to spawn sub-agents "liberally" and advertises a concurrency cap that is
   ~2.5× the real one. This is the "overlapping material that sets our agents up
   for failure."
3. **Make Goal Mode a real persistent loop** on top of the durable substrate
   (#3215 hub; #891, #1976, #2058, #2029).
4. **Gate /swarm** until 1–3 hold (#3218); ship it as honest UX over durable,
   nonblocking workers, not a prompt-only fanout button.

## 2. LAUNCH BLOCKER — the TUI freeze on sub-agent fanout (#3216 / #2211)

A live session froze (typing, render, cancel, sidebar all dead) when an agent
spawned ~6 sub-agents via `agent_open`. Root-caused at the **code level** (not just
the design-doc level). It is a **trigger × mechanism** failure.

### 2.1 Mechanism (CONFIRMED) — why it hard-freezes

The doc (`AGENT_RUNTIME.md:74-99`) blames "heavy per-child runtime clone + tool
registry rebuild + per-child transcript render." That is **real but secondary** —
it explains *lag* (tens of ms), not a multi-second *freeze with dead cancel*. The
actual hard-freeze chain:

- `agent_open` is **non-parallel** (approval-gated, no `supports_parallel`) →
  runs in the **Serial** tool batch, one `.await` at a time
  (`turn_loop.rs:1795-1796`).
- Each serial tool acquires the **process-global** `tool_exec_lock` **WRITE** guard
  and holds it across the entire tool body (`tool_execution.rs:322-326`; declared
  `engine.rs:542`).
- Inside that guarded body, `agent_open` calls the per-child **flash router**
  (`resolve_subagent_assignment_route` → `subagent_flash_router` →
  `client.create_message`) bounded only by a **4s** timeout
  (`subagent/mod.rs:3425-3431, 6204-6208`) — a real network model call under
  `/model auto` on a provider with a cheap tier.
- So the parent serially does **up to 6 × ~4s ≈ 24s** of work, each iteration
  holding the global tool write lock, **inline in the single Engine task**.
- The Engine task is stuck in `handle_send_message` and never returns to
  `rx_op.recv()` to read `Op::CancelRequest` (`engine.rs:1117, 1139, 1177`), **and
  the serial tool loop has no `cancel_token` select arm** (contrast the streaming/
  subagent-wait selects at `turn_loop.rs:406, 500, 1105`). → **cancel is provably
  dead** for the whole batch.
- Compounded by a **blocking `event::poll`/`event::read` inside the async render
  task** (`ui.rs:2857-2860`, not `spawn_blocking`), which under spawn-storm worker
  pressure degrades input/redraw.

**Lock & await audit (top offenders):**

| Lock | Site | Held across | Verdict |
|---|---|---|---|
| `tool_exec_lock` (RwLock) **WRITE** | acquire `tool_execution.rs:322-326` | whole tool body incl. 4s router model call + manager write + sync disk write (for `agent_open`) | **UNSAFE — primary** |
| `tx_event` bounded(256) `send().await` | `engine.rs:676`; many send sites | producer blocks when render drains slowly | **UNSAFE — secondary (backpressure amplifier)** |
| `SubAgentManager` RwLock WRITE | `subagent/mod.rs:3437` | sync `persist_state` disk I/O (no await), nested inside `tool_exec_lock` | Borderline (×6 serialized disk writes) |
| `rx_event` RwLock WRITE (render) | `ui.rs:1428` | non-blocking `try_recv` drain | Safe |
| `manager.try_write()` progress | `subagent/mod.rs:5018` | brief record | Safe (uses `try_write`) |

### 2.2 Trigger (CONFIRMED) — why the model spawns six at once

This is the "overlapping material" worry, verified in the prompts:

- **"Use them liberally for parallel work"** — `base.md:166`, `constitution.md:297`,
  `constitution.yaml:224`. The `{subagent_economics}` expansion frames sub-agents as
  cheap ("$0.14/M"), rewarding fanout.
- **Cap misinformation** — the prompt advertises **"defaults to 10 concurrent
  sub-agents … hard ceiling 20"** (`base.md:172`, `constitution.md:315`,
  `constitution.yaml:230`, `SUBAGENTS.md:131`). But 10/20 is the *records* cap
  (`max_subagents`); the **effective concurrent-launch limit is 4**
  (`DEFAULT_INTERACTIVE_LAUNCH_LIMIT`, applied `subagent/mod.rs:1597-1599`) — direct
  children beyond 4 **queue**. The model is coached to open up to 10 heavy
  in-process clones; 4 run, the rest queue, each clone is heavy, and the parent is
  meanwhile wedged on the global lock.
- A *third* fanout surface (`sub_query_batch`/`sub_query_sequence`, RLM) is also
  taught at `base.md:197-199`.

### 2.3 The fix (planned in chunk `freeze-fix`)

Failing-test-first, then minimal targeted fix:

1. **Six-worker stress harness** (deterministic, no network) with mixed worker
   states (model-wait, tool-wait, slow-success, retryable-timeout, hard-failure,
   immediate-complete). Two tests:
   - *Engine-level (decisive):* a mock non-parallel tool holding the global lock;
     assert `Op::CancelRequest` lands within a bounded deadline mid-batch. **Fails
     today** (cancel dead).
   - *Render/engine-tick liveness:* assert a tick counter / `last_*_at` timestamps
     keep advancing while six workers run.
2. **Cancel arm** on the serial tool-call loop (`turn_loop.rs:~1795`) so cancel/steer
   land mid-batch.
3. **Don't hold the global `tool_exec_lock` write guard across the router model
   call** — resolve the route before/without global exclusion; spawning a child
   does not need process-wide tool exclusion.
4. **Watchdog / debug-dump**: liveness timestamps on `App` (`last_render_at`,
   `render_tick`, `last_input_at`) and `Engine` (`last_op_serviced_at`,
   `current_turn_started_at`, `tool_exec_lock` held-since), plus a manager snapshot
   (running/pending, `launch_gate.available_permits()`), surfaced via a `/debug
   dump`. (Agent A §5.)
5. **Larger follow-up (separate PR):** per the runtime doc's cutover rule, make
   `agent_open` *enqueue/observe a fleet-backed worker run* for durable work instead
   of owning a heavy in-process lifecycle. Tracked as the #3096/#3216 completion.

## 3. Sub-agent vs Fleet — the "two moving targets" / overlapping material

Architecture is **~70% unified**. `codewhale exec` reuses the TUI `Engine`; Fleet
shells out to `codewhale exec` (the durable primitive); the recursion axis
(`DEFAULT_SPAWN_DEPTH`/`MAX_SPAWN_DEPTH_CEILING`, `config/src/lib.rs:1095-1114`) is
single-sourced. **But** the interactive `agent_open` path still uses the old
in-process clone engine and **never touches Fleet** (`subagent/mod.rs:1167` is just
a comment), and several overlaps remain.

| Concern | Sub-agent | Fleet | Status |
|---|---|---|---|
| Execution | in-process clone (`subagent/mod.rs:2165`, registry rebuild `6504-6540`) | subprocess `exec` (`fleet/executor.rs:40-79`) | Two impls; `agent_open` not yet on the durable one |
| Roles/prompts | `SubAgentType` + `*_AGENT_INTRO` (`mod.rs:379-427, 6798-6854`) | role strings → `SubAgentType` (`worker_runtime.rs:124-138`) | One authoritative source + one-way map; OK |
| Concurrency cap | semaphore **4** (`mod.rs:1597`) + prompt says **10/20** | scheduler 4/4/4 (`scheduler.rs:18-34`) | **Prompt mismatch — fix in chunk 2** |
| Events/lifecycle | `SubAgentStatus`/`AgentWorkerStatus` | `FleetWorkerEventPayload`/ledger | reconciled only via `exec` stream-json; `agent_status_to_fleet_event` is **dead** (`worker_runtime.rs:171`) |
| Persistence | `subagents.v1.json` | `fleet.jsonl` | interim duplicate (self-described) |

**Dead/orphan code** (verify-then-delete, own cleanup PR): `agent_status_to_fleet_event`
(`worker_runtime.rs:171`); fleet's in-process `register_worker` tracking stub
(`manager.rs:653-689`); superseded legacy sub-agent surfaces (`subagent/mod.rs:3718,
3809, 3869, 3960, 4078, 4176, 4272, 4420` — only the ones labeled "superseded";
several adjacent surfaces are LIVE registered tools, do **not** delete those).

## 4. Goal Mode — current state and plan (#3215 hub)

**There are three disconnected goal models today**, and the "loop" is within-turn
only:

- `HuntState` — TUI in-memory, not serialized (`app.rs:1188`).
- runtime `GoalState` — `Instant`-based, non-serializable, rebuilt every turn
  (`tools/goal.rs:69`); tools `create_goal`/`get_goal`/`update_goal`
  (`goal.rs:241,304,358`).
- durable `ThreadGoal` — fully durable SQLite (`state/lib.rs:525`,
  `protocol/lib.rs:67`) **but orphaned** from the runtime loop.

The continuation loop (`turn_loop.rs:2355`, capped at 3 passes/turn, resets each
turn) re-injects a self-audit prompt when the model stops emitting tool calls.
`/goal` and `/hunt` are already the **same** command (alias done). Token/time
fields (`tokens_used`, `time_used_seconds`, `UsageLimited`, `BudgetLimited`) exist
in protocol + SQLite but are **always 0 / never enforced**.

**Gap → plan:**

| Requirement | Today | Plan |
|---|---|---|
| Persistent re-invoke loop | within-turn only (3/turn, resets) | lift the continuation counter to durable cross-turn state; add a post-turn "re-dispatch if still active" decision |
| Durable state across restart | orphaned `ThreadGoal` exists | make the runtime read/write `ThreadGoal` (durable) instead of the `Instant`-based `GoalState` |
| LLM-as-judge completion (#2058) | self-audit only | gate `update_goal complete` behind the `Verifier` role / `run_verifiers` (`tools/verifier.rs:30`) |
| Explicit/durable scheduling | implicit `if` | persist a "next continuation" record; optionally ride the Fleet scheduler (`fleet/scheduler.rs:49`) for day-scale autonomy (#3154) |
| Steering while running | works in-process (`rx_steer`, `turn_loop.rs:86`) | reuse for live loop; mailbox for detached loop |
| Per-goal token/time accounting | dead fields | **increment** `tokens_used`/`time_used_seconds`; enforce `Usage/BudgetLimited` |

**Build seams:** continuation hook (`turn_loop.rs:2355`); durable store
(`state/lib.rs:737,784,801`); Fleet scheduler/ledger; Verifier role
(`subagent/mod.rs:398`); existing event plumbing (`engine.rs:1924` ↔ `ui.rs:5799`).

**Safe first slice (low risk):** start metering — increment the existing
`tokens_used`/`time_used_seconds` on `ThreadGoal` so the sidebar bar
(`sidebar.rs:566`) reflects *goal-scoped* usage rather than session-wide tokens.
This is additive and unblocks the "token/time accounting must be visible"
requirement without the larger model-unification surgery.

## 5. Chunked-PR plan (branches off `codex/v0.8.61`)

Each chunk is an isolated branch off `ae101a123`, sized for one reviewable PR.

| # | Branch | Scope | Risk | Status |
|---|---|---|---|---|
| 0 | `codex/v0.8.61-release-triage` | this doc | none | **this PR** |
| 1 | `codex/v0.8.61-subagent-prompt-honesty` | reframe "liberally" → deliberate; correct cap framing (effective ~4, batch-and-poll) in `base.md`, `constitution.md`, `constitution.yaml`, `SUBAGENTS.md` | low (prompt/docs) | see chunk |
| 2 | `codex/v0.8.61-freeze-fix` | six-worker stress harness (red) + cancel arm on serial loop + drop global lock across router call + watchdog/debug-dump | medium (engine turn loop) | see chunk |
| 3 | `codex/v0.8.61-subagent-deadcode` *(optional)* | remove dead `agent_status_to_fleet_event` + superseded labeled surfaces (verify-then-delete) | low | planned |
| 4 | `codex/v0.8.61-goal-metering` *(optional)* | Goal Mode first slice: wire token/time metering on durable `ThreadGoal` | low–med | planned |

Larger items (full `agent_open`→fleet cutover; full Goal Mode persistent loop;
permission profiles #3211/#3217; /swarm gating #3218) are **multi-PR efforts** —
designed here, to be built on top of chunks 1–2.

## 6. Issue reality-check (light pass — verify before closing)

| Issue | Ask | Likely already on branch? | Note |
|---|---|---|---|
| #3066 | non-DeepSeek cost tracking | No (gap verified) | merge PR #3201 → closes it |
| #3069 | rename DEEPSEEK_BLUE → WHALE_ACCENT_PRIMARY | No | PR #3197 (needs rebase) |
| #2960 (#3013) | legacy deepseek binary migration on update | **Yes** | `update.rs` already has it → close-with-credit |
| #2966 (#3195) | telegram keep polling while streaming | No (draft PR) | promote #3195 |
| #3096 | headless sub-agent worker runtime | Largely | close shipped slice w/ note; cutover remains (ties to #3216) |
| #3154 | Agent Fleet control plane | Largely | core shipped; endurance addenda open |
| #2211 | sub-agent fanout saturates TUI | Partial | **freeze-fix chunk addresses the core** |
| #1806 | sub-agent 120s timeout unusable | No | rolled into #3216/#3217 |
| #3216 | nonblocking fanout + freeze diagnosis | **In progress here** | freeze-fix chunk |
| #3203 | reliable queued steering + Ctrl+S | Partial | landed `5ca618d70` + Ctrl+S; relates PR #3170 |
| #3204 | context-window metadata + preflight | No (owner says keep open) | open |
| #3218 | gate /swarm | Partial | `/swarm` exists; readiness gate unconfirmed |

(Full table in the triage session log; #3211/#3213/#3217/#3188/#3075/#3076/#3073/
#3058/#3068 are net-new with no shipped signal.)

## 7. PR stewardship recommendations (for Hunter — NOT executed)

Mergeability verified by `git merge-tree` against the **actual** release head, not
GitHub's `main`-based flag. The release branch moved `commands/*.rs` →
`commands/groups/<group>/*.rs`, which drives most conflicts.

**Land (clean, high value):**
1. **#3201** (@mvanhorn) — non-DeepSeek pricing; clean vs branch, CI green, gap
   verified. **Closes #3066.** Highest value, lowest risk.
2. **#3199** (@gaord) — `PUT /v1/sessions`; clean, only failing check is `cargo
   fmt`. Harvest after a fmt pass.
3. **#3195** (@cyq1017, draft) — telegram polling; clean + green. Promote → merge.
   **Closes #2966.**
4. **#3206** (@VincentCorleone) — WeChat bridge; isolated new dir, clean. Scope call
   (in/out for 0.8.61); verify the token-gated check.

**Re-apply on branch (small; conflict only due to TUI churn), with credit:**
5. **#3197** (@nightt5879) — DEEPSEEK_BLUE rename. **Closes #3069.**
6. **#3170** (@Hmbown) — Ctrl+S steer; coordinate with the #3203 rework already on
   branch.

**Close-with-credit (already done on branch):**
7. **#3013** (@cyq1017) — legacy-binary migration already implemented in `update.rs`.

**Defer past 0.8.61 (overlap our active surfaces — would destabilize the release):**
8. **#2865** (@lmclaw, modernize prompts/agents), **#3193** (@dumbjack, Pro Plan
   routing), **#2933** (@cy2311, hippocampal memory + subagent errors), **#2239**
   (@gordonlu, i18n — already milestoned v0.8.65, needs rebase onto `groups/`),
   **#2486** (@AdityaVG13, whaleflow cost — branch already ships a whaleflow crate).
   Post a credited status note pointing at the rebase target.

**Dependabot:** group-merge the 4 release-action bumps + vitest (#2991/#2993/#2992/
#2994/#2996) after rebase (CI green; touches `release.yml` — dry-run the pipeline).
**Hold** #2998 (tailwind v3→v4 major; needs web migration); sanity-check #2999.

> All of the above are recommendations. Per `AGENTS.md`/`CLAUDE.md`, do not merge,
> close, comment, tag, or publish without Hunter's explicit approval. Positive,
> crediting comments only.

## 8. Verification status & what Hunter should manually QA

Per-chunk verification (fmt/build/test) is recorded in each chunk's commit message
and the session final report. The freeze fix in particular needs **manual TUI QA**:
build the TUI, start a session under `/model auto`, have the agent spawn 6
sub-agents, and confirm typing/render/cancel/sidebar stay responsive and Ctrl+C
cancels mid-fanout. The deterministic harness covers the mechanism, but the live
TUI interaction (the original repro) should be eyeballed before release.

## 9. Push / PR / merge commands (codex/v0.8.61 is local-only)

The remote has only `main`. To open real GitHub PRs against the release branch you
must first push it. Suggested (run when ready):

```sh
# from the main checkout (repo root)
git push -u origin codex/v0.8.61            # publish the release branch (==main today)
# push each chunk branch:
git push -u origin codex/v0.8.61-release-triage
git push -u origin codex/v0.8.61-subagent-prompt-honesty
git push -u origin codex/v0.8.61-freeze-fix
# open PRs against the release branch:
gh pr create --repo Hmbown/CodeWhale --base codex/v0.8.61 \
  --head codex/v0.8.61-freeze-fix --title "fix(tui): unfreeze sub-agent fanout" --body-file ...
```

Or, matching the established local-release-branch workflow, merge the chunk branches
into `codex/v0.8.61` locally after review:

```sh
git switch codex/v0.8.61
git merge --no-ff codex/v0.8.61-subagent-prompt-honesty
git merge --no-ff codex/v0.8.61-freeze-fix
```
