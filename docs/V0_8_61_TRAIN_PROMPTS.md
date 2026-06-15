# v0.8.61 — six parallel worktree prompts (one per train)

The runtime-control-plane work splits into **6 independent trains**. Run each in its own
git worktree (or hand each prompt to one agent with `isolation: worktree`). Trains touch
mostly disjoint subsystems, so they parallelize; within a train, work the issues in the
given order. **Land each train as its own branch/PR against `codex/v0.8.61`.**

**Thesis (don't reorder):** runtime control-plane correctness leads — Trains 1→2→3→4.
Train 5 (UI) and 6 (distribution) ride along and can run anytime. Train 4 (goal mode)
depends on Train 3 (durable nonblocking workers) being real.

## Common preamble — PREPEND this to each train prompt below

```
You are implementing one workstream of CodeWhale v0.8.61 in an ISOLATED git worktree.
CodeWhale is a Rust workspace (provider-agnostic terminal coding agent; first-class
DeepSeek + GLM/MiniMax/Moonshot/OpenAI/etc).

WORKTREE CONFINEMENT (do FIRST — this prevents a real bug we hit):
1. Run `pwd` and `git rev-parse --show-toplevel`; confirm you are inside a worktree
   (path contains `.claude/worktrees/`). That toplevel is YOUR repo.
2. Edit files ONLY under that toplevel (relative paths). NEVER write to an absolute
   path outside it (e.g. /Volumes/VIXinSSD/codewhale/crates/...) — that is a SEPARATE
   checkout and writing there corrupts it.
3. Before committing, `git status` and confirm every changed path is inside your worktree.

CONTEXT — read these first (they are in your worktree if it branched from codex/v0.8.61;
otherwise read the issue + the design doc the orchestrator pasted):
- docs/V0_8_61_ISSUE_COVERAGE.md — disposition + plan for every milestone issue.
- docs/V0_8_61_DESIGNS_BATCH1.md — code-ready designs for several of these issues.
- docs/AGENT_RUNTIME.md, docs/SUBAGENTS.md — the one-runtime model.

FOUNDATIONS ALREADY LANDED (build ON these, don't duplicate; several are #![allow(dead_code)]
awaiting their first consumer):
- worker_profile.rs: WorkerRuntimeProfile (role/permissions/shell/tools/model-route/depth/
  background) + derive_child() non-escalating intersection. (#3217/#3211/#3213/#414/#426/#1186)
- goal_loop.rs: decide_continuation() — wired into turn_loop goal continuation. (#3215)
- crates/state record_thread_goal_usage(): durable per-goal token/time accrual.
- model_registry.rs (#3071/#3073), provider_readiness.rs (#3083), context_budget.rs (#3086),
  provider_adapter.rs (#3084), resource_telemetry.rs (#2666), request_tuning.rs (#3024).
- Freeze fixes already landed: cancel-between-batches; parent no longer barriers on running
  sub-agents (should_hold_turn_for_subagents). Route isolation #3227 landed.

ETHOS (AGENTS.md): preserve first-class DeepSeek support + CodeWhale branding; treat
community work as evidence; review from code+tests not titles; positive crediting comments
only; do NOT tag/publish/release or merge/close without the maintainer's explicit approval.

RULES: keep each change scoped + safe; do NOT weaken/remove existing tests or touch
Cargo.toml/lock/version/README/CHANGELOG version files. Everything must COMPILE and your new
tests must pass: `cargo fmt`, then the focused tests for what you touched. Do NOT run
`cargo build --release` (CI does). Commit incrementally with messages ending `Refs: #N`.
If an issue is epic-scale or unsafe in one pass, land the smallest safe slice + tests and
mark the rest as a clearly-noted follow-up rather than forcing a broken change.

OUTPUT: per issue — status (implemented/partial/blocked), files, the exact test command +
result, a short summary, and risks. Then the branch name + head SHA(s) for harvesting.
```

---

## Train 1 — Route / model isolation  *(LEAD — bad routes poison every test)*
**Issues:** #3227 (DONE), #3205, #3204, #3213, #3071 (registry seeded), #3072, #3073, #3075, #3024 (support-map seeded), #3025, #2027, #1768.
**Outcome:** one route-effective model service; session-local provider/model state; correct
context-window metadata + preflight over-limit; per-role/scout-vs-synthesis model routing.
**Guidance:** build the route-effective model inventory/service (#3205) on `model_registry`
+ `provider_readiness` + `provider_adapter`; fix context-window metadata + over-limit
preflight (#3204) using `context_budget`; split model-facing capabilities from human mode
labels (#3213); migrate hard-coded model lists to `model_registry` (#3073) and hydrate it
with an offline cache (#3072); wire per-role + scout/synthesis model routes (#2027/#1768)
through `worker_profile::ModelRoute` and `request_tuning`. Add a route-resolution test matrix.

## Train 2 — Permissions + shell nonblocking
**Issues:** #3211, #3212, #1186, #2475, #1791, #1786, #1737. (+ harden the pre-existing
env-mutating config tests, e.g. `save_api_key_for_openrouter_writes_provider_table` does
`unsafe set_var` without serialization — add a serial guard / env restore.)
**Outcome:** first-class permission profiles; foreground/background shell semantics;
read-only shell parallelism; no pointless wait-blocks; typed persistent permission rules.
**Guidance:** wire `worker_profile` (PermissionSet/ShellPolicy) into the real approval/shell
path, replacing the shell boolean (#3211/#3217-permissions/#1186); default independent
shell + verifier work to background jobs (#3212); make synchronous tools (file_search/
grep_files/list_dir) cancellable so they don't block turn cancellation (#1791); reconcile
failed-shell stuck state + PID/work-queue hangs (#1737/#1786); fix the YOLO/MCP prompt
interruption (#2475). See design batch for #3212/#1786/#1737.

## Train 3 — Worker / fleet / sub-agent convergence  *(the heart of 0.8.61)*
**Issues:** #3096, #3154, #3166, #3167, #3216 (Bug A done), #3217 (profile seeded), #3226,
#2211, #1806, #1679, #2652, #719, #414/#426 (intersection seeded).
**Outcome:** "sub-agent" becomes UX language over fleet-style durable workers; the parent
stays responsive; workers have rich states, receipts, retries, tool profiles, and a
parent-visible interaction contract.
**Guidance (the two highest-value fixes, from the convergent analysis):**
- **Bug 1 (freeze):** decouple the input loop from the engine-event drain — move the
  blocking `event::poll()`/`event::read()` (ui.rs ~2859) onto its own thread feeding a
  channel; AND coalesce `AgentProgress` events to one redraw per agent per drain.
- **Bug 3 (the hang):** adopt the autonomous-to-completion contract — a sub-agent must never
  park on `input_rx.recv()` (`WaitingForUser`). If it lacks info it makes a sensible default
  and records the assumption, OR terminates with a structured `NeedsInput { question }`
  result on the existing completion channel, so the parent wakes and re-dispatches. Keep
  `agent_eval` send as best-effort steering only.
- **Bug 2 (status):** plumb the real `AgentWorkerStatus` (10 states) to the sidebar instead
  of hardcoding "running" (sidebar.rs ~2077); widen the tool-facing `SubAgentStatus` so
  `agent_eval`/`agent_open` expose Queued/ModelWait/WaitingForUser. Add #3226's
  parent-visible worker interaction contract (recommended action per worker).
- Wire `agent_open` to build a `WorkerRuntimeProfile` + (durable mode) enqueue a fleet worker
  run per docs/AGENT_RUNTIME.md (#3096/#3154/#3217). Fleet dogfood smoke (#3166), org chart
  (#3167). Add the six-worker stress harness asserting input/render/cancel stay live (#3216).

## Train 4 — Goal mode (before /swarm)  *(depends on Train 3 durable workers)*
**Issues:** #3215, #3218, #891, #1976, #2058, #2029.
**Outcome:** a persistent objective loop that survives turn boundaries, resumable + durable,
with visible token/time accounting; `/swarm` stays gated until the substrate is real.
**Guidance:** make `goal_loop` cross-turn — lift the continuation counter to durable state on
the `ThreadGoal` store, increment usage via `record_thread_goal_usage`, and re-dispatch a
worker turn toward the objective until `decide_continuation` says Stop. Bridge the three goal
models (HuntState / runtime GoalState / durable ThreadGoal) onto the durable one. Add a
verifier-as-judge gate before `update_goal complete` (#2058). Keep `/swarm` gated (#3218)
until Train 3 lands. Sub-agent checkpoint/continue across turns (#2029).

## Train 5 — Composer / steering / TUI clarity  *(parallel; rides along)*
**Issues:** #3203, #3224, #2054, #3194, #3188 (DONE), #3190, #2982, #3028, #3078, #963, #2666
(telemetry seeded).
**Outcome:** reliable queued steering + Ctrl+S; discoverable/configurable shortcuts; clear
busy/free state; clickable stop/inspect affordances; word-wrap; live token throughput.
**Guidance:** reliable queued steering + Ctrl+S send + Ctrl+Enter terminal-conflict handling
(#3203/#3224); composer queued-steer state labeling with row send-now/drop/clear (#2054);
helper-hint audit (#3194); a visible busy/idle indicator in the footer (#2982); click-to-act
on sidebar rows + stop targets (#3028); auto-clear completed sub-agent cards with TTL (#3078);
word-wrap truncation fix (#963); surface token throughput during streaming using
`resource_telemetry` (#3190/#2666).

## Train 6 — Distribution + release hygiene  *(separate hardening train)*
**Issues:** #3207, #3208 (DONE), #2960, #2924, #2917, #1067, #3214 (DONE), #3192.
**Outcome:** clean install/update paths; correct Linux artifact naming + glibc; branch
hygiene; ACP registry submission.
**Guidance:** Linux glibc requirement + `GLIBC_2.39 not found` (#3207/#1067) — build against
an older glibc or document the floor; rebrand update path (`deepseek update`/npm) (#2960/
#2924/#2917) — extend the legacy-binary detection already in `crates/cli/src/update.rs`; ACP
registry submission to agentclientprotocol/registry (#3192) — primarily an external PR +
adapter-completeness review. `branch-hygiene.sh` (#3214) already shipped.
