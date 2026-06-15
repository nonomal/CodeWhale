//! Goal loop orchestrator — the persistent-objective control layer (#3215, and
//! its lineage #891 / #1976 / #2058 / #2029).
//!
//! This is the **WhaleFlow goal layer**: the decision core that turns a one-shot
//! `/goal` into a persistent work loop. Given the durable goal status, the
//! accumulated usage (from the per-goal accounting wired in `crates/state`
//! `record_thread_goal_usage`), and a budget, it decides whether to **continue**
//! (re-dispatch another worker turn toward the objective) or **stop** with a
//! terminal status. It is the orchestrator in the WhaleFlow≈ultracode mapping —
//! the loop that fans work out to workers (`worker_profile`) and verifies before
//! committing.
//!
//! Scope: **foundation only**. This module is pure decision logic + types with
//! tests; it does NOT yet drive the engine. Today the continuation hook lives
//! inline in `core/engine/turn_loop.rs` (`goal_continuation_message_if_needed`,
//! capped at 3 passes per turn and reset each turn). Wiring this loop in — so a
//! goal persists across turns, reads/writes the durable `ThreadGoal`, and
//! enforces the budget circuit-breaker — is the follow-up that makes goal mode
//! real (#3215). Keeping the decision logic here, pure and tested, lets that
//! wiring be a thin, low-risk change.

#![allow(dead_code)] // foundation: the engine consumes this in a follow-up (#3215).

/// Terminal or active state of a persistent goal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalRunStatus {
    /// Still working toward the objective.
    Active,
    /// The objective was achieved (the model self-reported done and, ideally, a
    /// verifier confirmed — see `GoalGate`).
    Completed,
    /// The model reported it is blocked and needs the user.
    Blocked,
}

/// Why the loop stopped, for a terminal decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// Objective achieved.
    Completed,
    /// Model reported blocked.
    Blocked,
    /// Token budget exhausted.
    TokenBudget,
    /// Wall-clock budget exhausted.
    TimeBudget,
    /// Continuation circuit-breaker tripped (too many continuations without a
    /// terminal signal) — prevents a runaway loop.
    ContinuationLimit,
}

/// Accumulated, durable progress for a goal run. Mirrors the fields wired by
/// `crates/state` `record_thread_goal_usage` (tokens_used / time_used_seconds)
/// plus a continuation counter the loop maintains.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GoalProgress {
    pub tokens_used: u64,
    pub time_used_seconds: u64,
    pub continuations: u32,
}

/// The bound on a goal run. `None` fields are unbounded. `max_continuations` is
/// the safety circuit-breaker that always applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GoalBudget {
    pub token_budget: Option<u64>,
    pub time_budget_seconds: Option<u64>,
    pub max_continuations: u32,
}

impl GoalBudget {
    /// A sensible default circuit-breaker for an unattended persistent loop:
    /// no token/time cap, but bounded continuations so it cannot run away.
    pub const fn with_continuation_cap(max_continuations: u32) -> Self {
        Self {
            token_budget: None,
            time_budget_seconds: None,
            max_continuations,
        }
    }
}

/// The decision the loop makes after each worker turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContinuationDecision {
    /// Re-dispatch another turn toward the objective.
    Continue,
    /// Stop; the goal run is terminal.
    Stop(StopReason),
}

/// Decide whether a persistent goal run should continue after a turn.
///
/// Precedence (most authoritative first):
/// 1. A terminal model status (Completed / Blocked) ends the run.
/// 2. Budget/circuit-breaker limits end the run (so an Active-but-over-budget
///    run cannot continue).
/// 3. Otherwise continue.
///
/// `max_continuations` is checked as a hard breaker even when no token/time
/// budget is set — a persistent loop must never spin forever.
#[must_use]
pub fn decide_continuation(
    status: GoalRunStatus,
    progress: GoalProgress,
    budget: GoalBudget,
) -> ContinuationDecision {
    // 1. Terminal model signal wins.
    match status {
        GoalRunStatus::Completed => return ContinuationDecision::Stop(StopReason::Completed),
        GoalRunStatus::Blocked => return ContinuationDecision::Stop(StopReason::Blocked),
        GoalRunStatus::Active => {}
    }

    // 2. Budget / circuit-breaker. Continuation cap first (always applies).
    if progress.continuations >= budget.max_continuations {
        return ContinuationDecision::Stop(StopReason::ContinuationLimit);
    }
    if let Some(tokens) = budget.token_budget
        && progress.tokens_used >= tokens
    {
        return ContinuationDecision::Stop(StopReason::TokenBudget);
    }
    if let Some(secs) = budget.time_budget_seconds
        && progress.time_used_seconds >= secs
    {
        return ContinuationDecision::Stop(StopReason::TimeBudget);
    }

    // 3. Keep going.
    ContinuationDecision::Continue
}

/// Whether a stop reason represents success (Completed) vs. an early/forced exit.
/// Useful for the UI/status projection (#2666 token/time visibility).
#[must_use]
pub fn is_success(reason: StopReason) -> bool {
    matches!(reason, StopReason::Completed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unbounded(continuations_cap: u32) -> GoalBudget {
        GoalBudget::with_continuation_cap(continuations_cap)
    }

    #[test]
    fn completed_status_stops_with_success() {
        let d = decide_continuation(
            GoalRunStatus::Completed,
            GoalProgress::default(),
            unbounded(100),
        );
        assert_eq!(d, ContinuationDecision::Stop(StopReason::Completed));
        assert!(is_success(StopReason::Completed));
    }

    #[test]
    fn blocked_status_stops_without_success() {
        let d = decide_continuation(
            GoalRunStatus::Blocked,
            GoalProgress::default(),
            unbounded(100),
        );
        assert_eq!(d, ContinuationDecision::Stop(StopReason::Blocked));
        assert!(!is_success(StopReason::Blocked));
    }

    #[test]
    fn active_under_budget_continues() {
        let progress = GoalProgress {
            tokens_used: 10,
            time_used_seconds: 5,
            continuations: 2,
        };
        let budget = GoalBudget {
            token_budget: Some(1000),
            time_budget_seconds: Some(600),
            max_continuations: 50,
        };
        assert_eq!(
            decide_continuation(GoalRunStatus::Active, progress, budget),
            ContinuationDecision::Continue
        );
    }

    #[test]
    fn continuation_cap_breaks_a_runaway_loop_even_when_unbounded() {
        let progress = GoalProgress {
            continuations: 8,
            ..GoalProgress::default()
        };
        // No token/time budget set, but the circuit-breaker still trips.
        let d = decide_continuation(GoalRunStatus::Active, progress, unbounded(8));
        assert_eq!(d, ContinuationDecision::Stop(StopReason::ContinuationLimit));
    }

    #[test]
    fn token_budget_exhaustion_stops() {
        let progress = GoalProgress {
            tokens_used: 1000,
            continuations: 1,
            ..GoalProgress::default()
        };
        let budget = GoalBudget {
            token_budget: Some(1000),
            time_budget_seconds: None,
            max_continuations: 100,
        };
        assert_eq!(
            decide_continuation(GoalRunStatus::Active, progress, budget),
            ContinuationDecision::Stop(StopReason::TokenBudget)
        );
    }

    #[test]
    fn time_budget_exhaustion_stops() {
        let progress = GoalProgress {
            time_used_seconds: 601,
            continuations: 1,
            ..GoalProgress::default()
        };
        let budget = GoalBudget {
            token_budget: None,
            time_budget_seconds: Some(600),
            max_continuations: 100,
        };
        assert_eq!(
            decide_continuation(GoalRunStatus::Active, progress, budget),
            ContinuationDecision::Stop(StopReason::TimeBudget)
        );
    }

    #[test]
    fn terminal_status_outranks_remaining_budget() {
        // Completed wins even if there is plenty of budget left.
        let progress = GoalProgress::default();
        let budget = GoalBudget {
            token_budget: Some(1_000_000),
            time_budget_seconds: Some(86_400),
            max_continuations: 1000,
        };
        assert_eq!(
            decide_continuation(GoalRunStatus::Completed, progress, budget),
            ContinuationDecision::Stop(StopReason::Completed)
        );
    }
}
