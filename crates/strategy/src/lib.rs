//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! Economind Strategy Engine
//!
//! Defines the core strategy traits (`Identifier`, `Timer`, `Sizer`), the
//! pipeline composition engine, strategy configuration types, run result types,
//! and the top-level `run_strategy` orchestration function.
//!
//! Strategy plugin crates implement the traits; this crate hosts and orchestrates them.

pub mod config;
pub mod context;
pub mod ensemble;
pub mod orchestrator;
pub mod pipeline;
pub mod run;
pub mod stack;
pub mod traits;
pub mod voting;

// Flat re-exports of the most commonly used items.
pub use self::config::{CompositionMode, ExecutionMode, PluginSpec, StrategyConfig};
pub use self::context::StrategyContext;
pub use self::ensemble::{
    sharpe_from_equity, sortino_from_equity, EnsembleRunner, EnsembleRunnerBuilder,
    OptimisationObjective, OptimisationResult, WeightOptimizer,
};
pub use self::orchestrator::{run_strategy, run_strategy_multi, StrategyRunner};
pub use self::pipeline::{PipelineRunner, PipelineRunnerBuilder, TradeSignal};
pub use self::run::{PersistedSignal, RunStatus, StrategyRunResult};
pub use self::stack::{StrategyStack, StrategyStackBuilder};
pub use self::traits::{
    Candidate, Identifier, PositionSize, Sizer, Timer, TimingSignal, TradeDirection,
};
pub use self::voting::{VotingRunner, VotingRunnerBuilder};

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use economind_core::model::Symbol;
    use rust_decimal::prelude::FromStr;
    use rust_decimal::Decimal;
    use std::collections::HashMap;

    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    // ── Stub trait implementations ────────────────────────────────────────────
    //
    // These minimal stubs let us exercise PipelineRunner, StrategyStack,
    // VotingRunner, and EnsembleRunner without any real data sources.

    /// An Identifier that passes every symbol in the universe through unchanged.
    struct PassAllIdentifier;

    #[async_trait]
    impl Identifier for PassAllIdentifier {
        fn name(&self) -> &str {
            "pass-all"
        }
        async fn identify(&self, ctx: &StrategyContext) -> Vec<Candidate> {
            ctx.universe
                .iter()
                .map(|s| Candidate {
                    symbol: s.clone(),
                    score: 1.0,
                    metadata: HashMap::new(),
                })
                .collect()
        }
    }

    /// An Identifier that always returns an empty list (filters everything out).
    struct RejectAllIdentifier;

    #[async_trait]
    impl Identifier for RejectAllIdentifier {
        fn name(&self) -> &str {
            "reject-all"
        }
        async fn identify(&self, _ctx: &StrategyContext) -> Vec<Candidate> {
            vec![]
        }
    }

    /// A Timer that always returns a fixed score for every candidate.
    struct FixedScoreTimer(f64);

    #[async_trait]
    impl Timer for FixedScoreTimer {
        fn name(&self) -> &str {
            "fixed-score"
        }
        async fn score(&self, candidate: &Candidate, _ctx: &StrategyContext) -> TimingSignal {
            TimingSignal {
                candidate: candidate.clone(),
                score: self.0,
                direction: TradeDirection::Long,
                rationale: format!("fixed:{}", self.0),
            }
        }
    }

    /// A Sizer that always returns a fixed position size.
    struct FixedSizer(Decimal);

    #[async_trait]
    impl Sizer for FixedSizer {
        fn name(&self) -> &str {
            "fixed-sizer"
        }
        async fn size(&self, signal: &TimingSignal, _ctx: &StrategyContext) -> PositionSize {
            PositionSize {
                symbol: signal.candidate.symbol.clone(),
                shares: self.0,
                notional: self.0 * d("100"),
                portfolio_fraction: d("0.05"),
            }
        }
    }

    /// Build a minimal `StrategyContext` with the given universe symbols.
    fn ctx_with_universe(symbols: &[&str]) -> StrategyContext {
        StrategyContext {
            universe: symbols.iter().map(|s| Symbol::new(s)).collect(),
            bars: HashMap::new(),
            fundamentals: HashMap::new(),
            macro_data: HashMap::new(),
            open_positions: HashMap::new(),
            portfolio_value: d("100000"),
            available_cash: d("100000"),
            current_drawdown: Decimal::ZERO,
            regime: None,
            parameters: HashMap::new(),
        }
    }

    // ── CompositionMode ───────────────────────────────────────────────────────

    #[test]
    fn composition_mode_display() {
        assert_eq!(CompositionMode::Pipeline.to_string(), "pipeline");
        assert_eq!(CompositionMode::Voting.to_string(), "voting");
        assert_eq!(CompositionMode::Ensemble.to_string(), "ensemble");
    }

    #[test]
    fn composition_mode_equality() {
        assert_eq!(CompositionMode::Pipeline, CompositionMode::Pipeline);
        assert_ne!(CompositionMode::Pipeline, CompositionMode::Voting);
    }

    #[test]
    fn composition_mode_serialises_lowercase() {
        let json = serde_json::to_string(&CompositionMode::Voting).unwrap();
        assert_eq!(json, "\"voting\"");
        let back: CompositionMode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, CompositionMode::Voting);
    }

    // ── StrategyConfig ────────────────────────────────────────────────────────

    #[test]
    fn strategy_config_new_is_enabled_version_1() {
        let cfg = StrategyConfig::new(
            "test-strategy",
            CompositionMode::Pipeline,
            vec![],
            HashMap::new(),
        );
        assert_eq!(cfg.name, "test-strategy");
        assert!(cfg.enabled);
        assert_eq!(cfg.version, 1);
        assert_eq!(cfg.composition, CompositionMode::Pipeline);
    }

    #[test]
    fn strategy_config_parameters_roundtrip() {
        let mut params = HashMap::new();
        params.insert("fast_ema".to_string(), "12".to_string());
        params.insert("slow_ema".to_string(), "26".to_string());
        let cfg = StrategyConfig::new("ema-strategy", CompositionMode::Pipeline, vec![], params);
        assert_eq!(
            cfg.parameters.get("fast_ema").map(String::as_str),
            Some("12")
        );
        assert_eq!(
            cfg.parameters.get("slow_ema").map(String::as_str),
            Some("26")
        );
    }

    #[test]
    fn plugin_spec_fields() {
        let spec = PluginSpec {
            role: "timer".to_string(),
            name: "trend-follow".to_string(),
        };
        assert_eq!(spec.role, "timer");
        assert_eq!(spec.name, "trend-follow");
    }

    // ── PipelineRunner ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn pipeline_passthrough_no_timers_emits_all_candidates() {
        let runner = PipelineRunnerBuilder::new()
            .identifier(PassAllIdentifier)
            .sizer(FixedSizer(d("10")))
            .build();
        let ctx = ctx_with_universe(&["AAPL", "GOOG"]);
        let signals = runner.run(&ctx).await;
        assert_eq!(
            signals.len(),
            2,
            "all candidates should pass through with no timers"
        );
    }

    #[tokio::test]
    async fn pipeline_reject_all_identifier_emits_nothing() {
        let runner = PipelineRunnerBuilder::new()
            .identifier(RejectAllIdentifier)
            .sizer(FixedSizer(d("10")))
            .build();
        let ctx = ctx_with_universe(&["AAPL", "GOOG"]);
        let signals = runner.run(&ctx).await;
        assert!(
            signals.is_empty(),
            "reject-all identifier should produce no signals"
        );
    }

    #[tokio::test]
    async fn pipeline_timer_above_threshold_emits_signal() {
        let runner = PipelineRunnerBuilder::new()
            .identifier(PassAllIdentifier)
            .timer(FixedScoreTimer(0.9)) // well above default 0.5 threshold
            .sizer(FixedSizer(d("5")))
            .score_threshold(0.5)
            .build();
        let ctx = ctx_with_universe(&["MSFT"]);
        let signals = runner.run(&ctx).await;
        assert_eq!(signals.len(), 1);
        assert!((signals[0].timing.score - 0.9).abs() < 1e-9);
        assert_eq!(signals[0].size.shares, d("5"));
    }

    #[tokio::test]
    async fn pipeline_timer_below_threshold_emits_nothing() {
        let runner = PipelineRunnerBuilder::new()
            .identifier(PassAllIdentifier)
            .timer(FixedScoreTimer(0.2)) // below 0.5 threshold
            .sizer(FixedSizer(d("5")))
            .score_threshold(0.5)
            .build();
        let ctx = ctx_with_universe(&["TSLA"]);
        let signals = runner.run(&ctx).await;
        assert!(signals.is_empty(), "low timer score should be filtered out");
    }

    #[tokio::test]
    async fn pipeline_averages_two_timer_scores() {
        // Two timers: 0.8 and 0.6 → average 0.7, which clears 0.5 threshold.
        let runner = PipelineRunnerBuilder::new()
            .identifier(PassAllIdentifier)
            .timer(FixedScoreTimer(0.8))
            .timer(FixedScoreTimer(0.6))
            .sizer(FixedSizer(d("10")))
            .score_threshold(0.5)
            .build();
        let ctx = ctx_with_universe(&["NVDA"]);
        let signals = runner.run(&ctx).await;
        assert_eq!(signals.len(), 1);
        assert!(
            (signals[0].timing.score - 0.7).abs() < 1e-9,
            "score={}",
            signals[0].timing.score
        );
    }

    #[tokio::test]
    async fn pipeline_empty_universe_produces_no_signals() {
        let runner = PipelineRunnerBuilder::new()
            .identifier(PassAllIdentifier)
            .timer(FixedScoreTimer(0.9))
            .sizer(FixedSizer(d("10")))
            .build();
        let ctx = ctx_with_universe(&[]);
        let signals = runner.run(&ctx).await;
        assert!(signals.is_empty());
    }

    // ── StrategyStack ─────────────────────────────────────────────────────────

    fn make_stack(timer_score: f64, threshold: f64) -> StrategyStack {
        StrategyStackBuilder::new("test-stack")
            .identifier(PassAllIdentifier)
            .timer(FixedScoreTimer(timer_score))
            .sizer(FixedSizer(d("10")))
            .score_threshold(threshold)
            .build()
    }

    #[tokio::test]
    async fn strategy_stack_emits_signal_when_score_above_threshold() {
        let stack = make_stack(0.8, 0.5);
        let ctx = ctx_with_universe(&["AAPL"]);
        let signals = stack.run(&ctx).await;
        assert_eq!(signals.len(), 1);
    }

    #[tokio::test]
    async fn strategy_stack_filters_when_score_below_threshold() {
        let stack = make_stack(0.3, 0.5);
        let ctx = ctx_with_universe(&["AAPL"]);
        let signals = stack.run(&ctx).await;
        assert!(signals.is_empty());
    }

    #[tokio::test]
    async fn strategy_stack_name_in_rationale() {
        let stack = make_stack(0.9, 0.0);
        let ctx = ctx_with_universe(&["AAPL"]);
        let signals = stack.run(&ctx).await;
        assert!(!signals.is_empty());
        // Stack name should appear in the rationale string
        assert!(
            signals[0].timing.rationale.contains("test-stack"),
            "rationale='{}' should contain stack name",
            signals[0].timing.rationale
        );
    }

    // ── VotingRunner ──────────────────────────────────────────────────────────

    #[test]
    #[should_panic(expected = "at least one stack")]
    fn voting_runner_panics_on_empty_stacks() {
        VotingRunner::new(vec![], 0.5);
    }

    #[test]
    #[should_panic(expected = "quorum must be")]
    fn voting_runner_panics_on_zero_quorum() {
        let stack = make_stack_sync(0.9, 0.5);
        VotingRunner::new(vec![stack], 0.0);
    }

    #[test]
    #[should_panic(expected = "quorum must be")]
    fn voting_runner_panics_on_quorum_above_one() {
        let stack = make_stack_sync(0.9, 0.5);
        VotingRunner::new(vec![stack], 1.1);
    }

    #[tokio::test]
    async fn voting_runner_unanimous_emits_signal() {
        // 2 stacks, quorum=0.5 → 2/2 = 100% ≥ 50% → emit
        let ctx = ctx_with_universe(&["AAPL"]);
        let runner = VotingRunner::new(vec![make_stack(0.9, 0.5), make_stack(0.8, 0.5)], 0.5);
        let signals = runner.run(&ctx).await;
        assert_eq!(signals.len(), 1, "unanimous vote should emit signal");
    }

    #[tokio::test]
    async fn voting_runner_below_quorum_suppresses_signal() {
        // 2 stacks; first votes (score=0.9>0.5), second does NOT vote (score=0.2<0.5).
        // 1/2 = 50%, quorum=0.6 → no signal.
        let ctx = ctx_with_universe(&["AAPL"]);
        let runner = VotingRunner::new(vec![make_stack(0.9, 0.5), make_stack(0.2, 0.5)], 0.6);
        let signals = runner.run(&ctx).await;
        assert!(
            signals.is_empty(),
            "below-quorum vote should produce no signal"
        );
    }

    #[tokio::test]
    async fn voting_runner_empty_universe_produces_no_signals() {
        let ctx = ctx_with_universe(&[]);
        let runner = VotingRunner::new(vec![make_stack(0.9, 0.5)], 0.5);
        let signals = runner.run(&ctx).await;
        assert!(signals.is_empty());
    }

    // ── EnsembleRunner ────────────────────────────────────────────────────────

    #[test]
    #[should_panic(expected = "at least one stack")]
    fn ensemble_runner_panics_on_empty_stacks() {
        EnsembleRunner::new(vec![], 0.5);
    }

    #[test]
    #[should_panic(expected = "weight must be")]
    fn ensemble_runner_panics_on_all_zero_weights() {
        let stack = make_stack_sync(0.9, 0.5);
        EnsembleRunner::new(vec![(stack, 0.0)], 0.5);
    }

    #[tokio::test]
    async fn ensemble_runner_normalises_weights() {
        // weights 1.0 + 1.0 → both become 0.5
        let ctx = ctx_with_universe(&["AAPL"]);
        let runner = EnsembleRunner::new(
            vec![(make_stack(0.8, 0.0), 1.0), (make_stack(0.6, 0.0), 1.0)],
            0.0,
        );
        let total_weight: f64 = runner.stacks.iter().map(|(_, w)| *w).sum();
        assert!(
            (total_weight - 1.0).abs() < 1e-9,
            "weights should sum to 1.0"
        );
        // Weighted score = 0.5*0.8 + 0.5*0.6 = 0.7 > 0.0 → signal emitted
        let signals = runner.run(&ctx).await;
        assert_eq!(signals.len(), 1);
    }

    #[tokio::test]
    async fn ensemble_runner_below_threshold_suppresses_signal() {
        // Single stack with score 0.3, threshold 0.5 → no signal
        let ctx = ctx_with_universe(&["AAPL"]);
        let runner = EnsembleRunner::new(vec![(make_stack(0.3, 0.0), 1.0)], 0.5);
        let signals = runner.run(&ctx).await;
        assert!(
            signals.is_empty(),
            "score 0.3 < threshold 0.5 should produce no signal"
        );
    }

    // ── sharpe_from_equity / sortino_from_equity ──────────────────────────────

    #[test]
    fn sharpe_positive_for_upward_trend() {
        let equity: Vec<f64> = (0..100).map(|i| 100_000.0 + i as f64 * 100.0).collect();
        let s = sharpe_from_equity(&equity);
        assert!(s > 0.0, "sharpe={}", s);
    }

    #[test]
    fn sharpe_neg_infinity_for_single_data_point() {
        // sharpe is undefined with < 2 data points — returns NEG_INFINITY
        let s = sharpe_from_equity(&[100_000.0]);
        assert!(
            s.is_infinite() && s < 0.0,
            "sharpe should be NEG_INFINITY for insufficient data"
        );
    }

    #[test]
    fn sharpe_empty_returns_neg_infinity() {
        let s = sharpe_from_equity(&[]);
        assert!(s.is_infinite() && s < 0.0);
    }

    #[test]
    fn sortino_neg_infinity_when_no_downside_returns() {
        // All positive returns → downside variance = 0 → NEG_INFINITY (undefined)
        let equity: Vec<f64> = (0..50).map(|i| 100_000.0 + i as f64 * 50.0).collect();
        let s = sortino_from_equity(&equity);
        assert!(
            s.is_infinite() && s < 0.0,
            "no downside returns should give NEG_INFINITY, got {s}"
        );
    }

    #[test]
    fn sortino_positive_for_mixed_returns() {
        // Mix of gains and small losses → positive sortino
        let mut equity = vec![100_000.0_f64];
        for i in 0..100 {
            let prev = *equity.last().unwrap();
            let ret = if i % 5 == 0 { -0.002 } else { 0.005 };
            equity.push(prev * (1.0 + ret));
        }
        let s = sortino_from_equity(&equity);
        assert!(
            s > 0.0,
            "positive-edge series should have positive sortino, got {s}"
        );
    }

    // ── Helper for sync test contexts (non-async) ─────────────────────────────

    fn make_stack_sync(timer_score: f64, threshold: f64) -> StrategyStack {
        make_stack(timer_score, threshold)
    }
}
