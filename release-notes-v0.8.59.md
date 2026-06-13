v0.8.59

CodeWhale v0.8.59 is a stability and integration release that hardens the TUI,
improves sidebar interactivity, localizes notifications, cleans up user-facing
naming, and adds experimental config and runtime API foundations.

---

## TUI stability and interactivity

- **Sidebar resize stays live during active turns** — the split pane no longer
  freezes while the model is generating.
- **Sidebar hover stays live while loading** — detail popovers update in real
  time instead of dropping mid-turn.
- **Hover highlight cleared on exit** — stale highlight state no longer
  persists after leaving the sidebar.
- **Ghostty motion override kept live** — mouse motion events continue to
  work correctly in Ghostty terminals.
- **Mouse-report sanitizer added** — raw SGR mouse reports are sanitized
  defensively so corrupted composer input is blocked.
- **Sidebar Copy action** — the right-click context menu now includes Copy
  alongside existing actions.
- **Richer Work overflow and Agents hover detail** — sidebar detail
  popovers surface more information for Work and Agents entries.
- **Provider-wait state is now observable** — the TUI shows route, idle
  budget, and fanout-preflight state during provider waits.
- **fanout launches are queued behind a visible launch gate** — interactive
  fanout no longer stalls the TUI without feedback.

## Sub-agent safety and worker-ledger substrate

- **Sub-agents survive backgrounded waits** — worker cards are no longer
  dropped when the TUI yields during long waits.
- **Interrupted sub-agent lifecycle events are emitted** — stale running
  cards are reconciled with actual state.
- **Runtime-prompt stand-still guard** — the TUI no longer spins in an
  autonomous loop when the launch gate is closed.

## i18n and user-facing naming

- **Notifications are now localized** — notification text respects the
  configured language instead of always rendering in English.
- **Tool family labels are localized** — 10 tool family labels now use
  MessageId-based i18n.
- **Config editor labels are localized** — config section editor labels
  follow the configured language.
- **"Bash" shown in user-facing UI** — shell execution is surfaced as "Bash"
  in the TUI while `exec_shell` remains the internal tool name.

## Provider and model updates

- **Kimi OAuth credentials aligned with Kimi Code** — the config path for
  Kimi OAuth credentials now matches the Kimi Code provider surface.
- **Kimi K2.7 Code defaults added** — model metadata for Kimi K2.7 Code is
  included.
- **SiliconFlow CN provider config split** — a separate provider entry for
  SiliconFlow China is available.
- **Provider metadata registry refactored** — provider metadata is now
  data-driven and easier to extend.
- **OpenRouter Nemotron preset fixed** — the invalid model ID is corrected.
- **Provider fallback chain activated** — harvested from community PR #2773.

## Experimental config and runtime API

- **Experimental feature flags** — `[experimental]` config section for goal
  and WhaleFlow opt-ins, surfaced through normal config paths.
- **Runtime API Phase 0 + Phase 1** — brand-neutral naming, capabilities
  advertisement, and dynamic tool protocol types for editor/GUI clients.
- **Command strategy registry** — harvested from community PR #2851.
- **Context source map report** — visibility into rules, tools, memory, and
  skills contributions to prompt cost.

## Community harvests

- **PR #3010** — lock slim default prompt with calm-overlay regression test.
- **PR #2808** — thread undo/retry and snapshot restore endpoints.
- **PR #3051** — voice input commands and hotbar integration.
- **PR #2773** — activate provider fallback chain.
- **PR #2851** — command strategy registry.

## Other fixes and improvements

- **macOS Command modifier normalized to Control** for keyboard shortcuts
  (#2938).
- **Hotbar slots dispatched from number keys**.
- **Thread goals persisted through the app server**.
- **Concise verbosity mode added** to config.
- **Workspace trust required for project hooks** — safety boundary
  enforced.
- **Thread detail item reads batched** — N+1 query fix.
- **Legacy deepseek users guided to codewhale** in update paths.
- **Static Linux x64 musl binaries** now built.
- **Approval rule metadata exposed at runtime**.
- **Codex response errors clarified** in TUI.
- **Microsoft Build Tools / cmake --build** shell compatibility fix.
- **PDF extraction hardened** for non-Identity-H CMap fonts.

---

**Full headless sub-agents (fleet manager, worker runtime, durable inbox/ledger)
are deferred to v0.8.60 / #3096.** The v0.8.59 release includes the
sub-agent safety and worker-ledger substrate that v0.8.60 builds on.

**v0.8.60+ tracking issues remain open:**
- #3096 — Full headless sub-agents and worker runtime
- #1310 — MiniMax first-party provider
- #3187 — Z.ai / StepFlash first-party providers
