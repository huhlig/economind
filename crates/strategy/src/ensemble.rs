//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! Ensemble / Weighted composition engine (§8.B).
//!
//! `EnsembleRunner` takes a collection of `(StrategyStack, weight)` pairs.
//! It runs all stacks against the same `StrategyContext`, then computes a
//! weighted sum of each instrument's signal scores.  Instruments whose
//! weighted score meets or exceeds `signal_threshold` are emitted as signals.
//!
//! ## Weight semantics
//!
//! Weights are non-negative floats and are normalised internally so they
//! always sum to 1.0.  A weight of 0.0 effectively disables the stack.
//!
//! ## Signal assembly
//!
//! The emitted signal's timing score is the normalised weighted average.
//! The position size is computed by the Sizer of the *highest-weight* stack
//! that voted for the instrument (ties broken by order).
//!
//! ## Weight optimisation (§8.B.2)
//!
//! `WeightOptimizer` implements a Nelder-Mead simplex search over the weight
//! vector, maximising a chosen objective (default: Sharpe ratio) over a
//! provided backtest equity curve.  The search operates on the normalised
//! simplex and returns the optimal weight vector.

use crate::context::StrategyContext;
use crate::pipeline::TradeSignal;
use crate::stack::StrategyStack;
use crate::traits::{Candidate, PositionSize, TimingSignal, TradeDirection};
use economind_core::model::Symbol;
use rust_decimal::Decimal;
use std::collections::HashMap;

// ── EnsembleRunner ────────────────────────────────────────────────────────────

/// Runs weighted strategy stacks and emits ensemble-consensus signals.
pub struct EnsembleRunner {
    /// (stack, normalised weight) pairs.  Weights sum to 1.0.
    pub stacks: Vec<(StrategyStack, f64)>,
    /// Minimum weighted score to emit a signal (0.0–1.0).
    pub signal_threshold: f64,
}

impl EnsembleRunner {
    /// Create a new `EnsembleRunner`.
    ///
    /// Weights are normalised automatically.
    ///
    /// # Panics
    /// - If `stacks` is empty.
    /// - If all weights are zero.
    pub fn new(stacks: Vec<(StrategyStack, f64)>, signal_threshold: f64) -> Self {
        assert!(
            !stacks.is_empty(),
            "EnsembleRunner requires at least one stack"
        );
        let total: f64 = stacks.iter().map(|(_, w)| *w).sum();
        assert!(total > 0.0, "At least one stack weight must be > 0.0");

        let normalised: Vec<(StrategyStack, f64)> =
            stacks.into_iter().map(|(s, w)| (s, w / total)).collect();

        Self {
            stacks: normalised,
            signal_threshold,
        }
    }

    /// Run all stacks and return weighted ensemble `TradeSignal`s.
    pub async fn run(&self, ctx: &StrategyContext) -> Vec<TradeSignal> {
        // Collect per-instrument contributions: symbol → Vec<(weight, TimingSignal, PositionSize)>
        let mut contributions: HashMap<Symbol, Vec<(f64, TimingSignal, PositionSize)>> =
            HashMap::new();

        for (stack, weight) in &self.stacks {
            if *weight <= 0.0 {
                continue;
            }
            let stack_signals = stack.run(ctx).await;
            for ts in stack_signals {
                contributions
                    .entry(ts.timing.candidate.symbol.clone())
                    .or_default()
                    .push((*weight, ts.timing, ts.size));
            }
        }

        // Compute weighted scores and emit signals above threshold.
        let mut signals: Vec<TradeSignal> = Vec::new();

        for (symbol, contribs) in contributions {
            // Weighted sum of timing scores (weights are already normalised to
            // sum to 1.0 across all stacks; here we re-normalise by the sum of
            // weights *that actually produced a vote* so abstaining stacks don't
            // dilute the score).
            let total_weight: f64 = contribs.iter().map(|(w, _, _)| *w).sum();
            let weighted_score: f64 =
                contribs.iter().map(|(w, t, _)| w * t.score).sum::<f64>() / total_weight;

            if weighted_score < self.signal_threshold {
                continue;
            }

            // Weighted identifier score.
            let weighted_id_score: f64 = contribs
                .iter()
                .map(|(w, t, _)| w * t.candidate.score)
                .sum::<f64>()
                / total_weight;

            // Direction: use the direction from the highest-weight contributor.
            let direction = contribs
                .iter()
                .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(_, t, _)| t.direction)
                .unwrap_or(TradeDirection::Long);

            // Position size: weighted average of contributing stacks.
            let avg_shares = avg_decimal_weighted(contribs.iter().map(|(w, _, s)| (*w, s.shares)));
            let avg_notional =
                avg_decimal_weighted(contribs.iter().map(|(w, _, s)| (*w, s.notional)));
            let avg_fraction =
                avg_decimal_weighted(contribs.iter().map(|(w, _, s)| (*w, s.portfolio_fraction)));

            let contributing_stacks: Vec<&str> = self
                .stacks
                .iter()
                .filter(|(stack, _)| {
                    contribs
                        .iter()
                        .any(|(_, t, _)| t.rationale.contains(&format!("[{}]", stack.name)))
                })
                .map(|(stack, _)| stack.name.as_str())
                .collect();

            let rationale = format!(
                "Ensemble: weighted score={:.3} (threshold={:.2}); contributors={}",
                weighted_score,
                self.signal_threshold,
                if contributing_stacks.is_empty() {
                    format!("{} stacks", contribs.len())
                } else {
                    contributing_stacks.join(", ")
                },
            );

            signals.push(TradeSignal {
                timing: TimingSignal {
                    candidate: Candidate {
                        symbol: symbol.clone(),
                        score: weighted_id_score,
                        metadata: {
                            let mut m = HashMap::new();
                            m.insert("ensemble_score".to_string(), format!("{weighted_score:.4}"));
                            m.insert(
                                "contributing_stacks".to_string(),
                                contribs.len().to_string(),
                            );
                            m
                        },
                    },
                    score: weighted_score,
                    direction,
                    rationale,
                },
                size: PositionSize {
                    symbol,
                    shares: avg_shares,
                    notional: avg_notional,
                    portfolio_fraction: avg_fraction,
                },
            });
        }

        // Sort descending by weighted score.
        signals.sort_by(|a, b| {
            b.timing
                .score
                .partial_cmp(&a.timing.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        signals
    }

    /// Return a copy of the current (normalised) weight vector.
    pub fn weights(&self) -> Vec<f64> {
        self.stacks.iter().map(|(_, w)| *w).collect()
    }
}

// ── WeightOptimizer ───────────────────────────────────────────────────────────

/// Objective function for weight optimisation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptimisationObjective {
    /// Maximise annualised Sharpe ratio.
    Sharpe,
    /// Maximise Sortino ratio (downside deviation only).
    Sortino,
    /// Maximise CAGR.
    Cagr,
}

/// Result of a weight optimisation run.
#[derive(Debug, Clone)]
pub struct OptimisationResult {
    /// Optimal normalised weight vector (sums to 1.0).
    pub weights: Vec<f64>,
    /// Best objective value achieved.
    pub objective_value: f64,
    /// Number of function evaluations.
    pub evaluations: usize,
    /// Whether the optimiser converged within the iteration budget.
    pub converged: bool,
}

/// Nelder-Mead simplex optimiser for ensemble weights.
///
/// Operates on the *probability simplex* — the set of non-negative vectors that
/// sum to 1.0.  The search is unconstrained in a `(n-1)`-dimensional space and
/// re-projected onto the simplex at each evaluation.
///
/// # Usage
///
/// ```ignore
/// let result = WeightOptimizer::new(n_stacks)
///     .objective(OptimisationObjective::Sharpe)
///     .max_iterations(500)
///     .optimise(|weights| score_weights(weights, &equity_curves));
/// ```
pub struct WeightOptimizer {
    n: usize,
    objective: OptimisationObjective,
    max_iterations: usize,
    tolerance: f64,
}

impl WeightOptimizer {
    pub fn new(n_stacks: usize) -> Self {
        assert!(n_stacks >= 2, "Need at least 2 stacks to optimise weights");
        Self {
            n: n_stacks,
            objective: OptimisationObjective::Sharpe,
            max_iterations: 1000,
            tolerance: 1e-6,
        }
    }

    pub fn objective(mut self, obj: OptimisationObjective) -> Self {
        self.objective = obj;
        self
    }

    pub fn max_iterations(mut self, n: usize) -> Self {
        self.max_iterations = n;
        self
    }

    pub fn tolerance(mut self, tol: f64) -> Self {
        self.tolerance = tol;
        self
    }

    /// Run the Nelder-Mead search.
    ///
    /// `score_fn` is called with a normalised weight vector and must return the
    /// objective value (higher = better).  The optimiser **maximises** the
    /// objective.
    pub fn optimise<F>(&self, score_fn: F) -> OptimisationResult
    where
        F: Fn(&[f64]) -> f64,
    {
        let n = self.n;

        // Work in (n-1)-dimensional unconstrained space; project back to simplex.
        // Initial simplex: equal weights + small perturbations.
        let init: Vec<f64> = vec![1.0 / n as f64; n];
        let mut simplex = self.build_initial_simplex(&init);
        let mut f_vals: Vec<f64> = simplex.iter().map(|v| score_fn(v)).collect();

        let alpha = 1.0_f64; // reflection
        let gamma = 2.0_f64; // expansion
        let rho = 0.5_f64; // contraction
        let sigma = 0.5_f64; // shrink

        let mut evals = simplex.len();
        let mut converged = false;

        for _iter in 0..self.max_iterations {
            // Sort ascending (we negate so lower = better for minimiser).
            let mut order: Vec<usize> = (0..simplex.len()).collect();
            order.sort_by(|&a, &b| {
                f_vals[b]
                    .partial_cmp(&f_vals[a])
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            // order[0] = best, order[last] = worst.

            // Convergence check: range of objective values.
            let f_best = f_vals[order[0]];
            let f_worst = f_vals[*order.last().unwrap()];
            if (f_worst - f_best).abs() < self.tolerance {
                converged = true;
                break;
            }

            // Centroid (exclude worst).
            let centroid = self.centroid(&simplex, &order[..order.len() - 1]);

            // Reflection.
            let worst_idx = *order.last().unwrap();
            let reflected = self.project_simplex(&self.add_scaled(
                &centroid,
                &self.sub(&centroid, &simplex[worst_idx]),
                alpha,
            ));
            let f_reflected = score_fn(&reflected);
            evals += 1;

            if f_reflected > f_vals[order[0]] {
                // Expansion.
                let expanded = self.project_simplex(&self.add_scaled(
                    &centroid,
                    &self.sub(&reflected, &centroid),
                    gamma,
                ));
                let f_expanded = score_fn(&expanded);
                evals += 1;
                if f_expanded > f_reflected {
                    simplex[worst_idx] = expanded;
                    f_vals[worst_idx] = f_expanded;
                } else {
                    simplex[worst_idx] = reflected;
                    f_vals[worst_idx] = f_reflected;
                }
            } else if f_reflected >= f_vals[order[order.len() - 2]] {
                simplex[worst_idx] = reflected;
                f_vals[worst_idx] = f_reflected;
            } else {
                // Contraction.
                let use_reflected = f_reflected > f_vals[worst_idx];
                let contract_point = if use_reflected {
                    &reflected
                } else {
                    &simplex[worst_idx].clone()
                };
                let contracted = self.project_simplex(&self.add_scaled(
                    &centroid,
                    &self.sub(contract_point, &centroid),
                    rho,
                ));
                let f_contracted = score_fn(&contracted);
                evals += 1;

                if f_contracted > f_vals[worst_idx] {
                    simplex[worst_idx] = contracted;
                    f_vals[worst_idx] = f_contracted;
                } else {
                    // Shrink: move all vertices toward the best.
                    let best = simplex[order[0]].clone();
                    for i in 1..simplex.len() {
                        let idx = order[i];
                        simplex[idx] = self.project_simplex(&self.add_scaled(
                            &best,
                            &self.sub(&simplex[idx], &best),
                            sigma,
                        ));
                        f_vals[idx] = score_fn(&simplex[idx]);
                        evals += 1;
                    }
                }
            }
        }

        // Return best found.
        let best_idx = f_vals
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| i)
            .unwrap_or(0);

        OptimisationResult {
            weights: simplex[best_idx].clone(),
            objective_value: f_vals[best_idx],
            evaluations: evals,
            converged,
        }
    }

    // ── Simplex helpers ───────────────────────────────────────────────────────

    fn build_initial_simplex(&self, init: &[f64]) -> Vec<Vec<f64>> {
        let n = self.n;
        let mut simplex = Vec::with_capacity(n + 1);
        simplex.push(self.project_simplex(init));
        let step = 0.05_f64.max(1.0 / (n as f64 * 4.0));
        for i in 0..n {
            let mut perturbed = init.to_vec();
            perturbed[i] += step;
            // Adjust one other element to keep roughly on simplex.
            let j = (i + 1) % n;
            perturbed[j] -= step;
            simplex.push(self.project_simplex(&perturbed));
        }
        simplex
    }

    /// Project an arbitrary vector onto the probability simplex (non-negative,
    /// sums to 1).  Uses the O(n log n) algorithm by Duchi et al. (2008).
    fn project_simplex(&self, v: &[f64]) -> Vec<f64> {
        let _n = v.len();
        let mut u: Vec<f64> = v.to_vec();
        u.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));

        let mut cssv = 0.0_f64;
        let mut rho = 0_usize;
        for (j, &uj) in u.iter().enumerate() {
            cssv += uj;
            if uj - (cssv - 1.0) / (j as f64 + 1.0) > 0.0 {
                rho = j;
            }
        }
        let theta = (u[..=rho].iter().sum::<f64>() - 1.0) / (rho as f64 + 1.0);
        v.iter().map(|&x| (x - theta).max(0.0)).collect()
    }

    fn centroid(&self, simplex: &[Vec<f64>], indices: &[usize]) -> Vec<f64> {
        let mut c = vec![0.0_f64; self.n];
        for &i in indices {
            for (j, &v) in simplex[i].iter().enumerate() {
                c[j] += v;
            }
        }
        let k = indices.len() as f64;
        c.iter_mut().for_each(|x| *x /= k);
        c
    }

    fn add_scaled(&self, base: &[f64], delta: &[f64], scale: f64) -> Vec<f64> {
        base.iter()
            .zip(delta.iter())
            .map(|(b, d)| b + scale * d)
            .collect()
    }

    fn sub(&self, a: &[f64], b: &[f64]) -> Vec<f64> {
        a.iter().zip(b.iter()).map(|(x, y)| x - y).collect()
    }
}

// ── scoring helpers for equity curves ─────────────────────────────────────────

/// Compute annualised Sharpe ratio from a daily equity curve.
///
/// `equity` must be sorted oldest → newest.  Returns `f64::NEG_INFINITY` if
/// there are fewer than 2 observations.
pub fn sharpe_from_equity(equity: &[f64]) -> f64 {
    if equity.len() < 2 {
        return f64::NEG_INFINITY;
    }
    let returns: Vec<f64> = equity.windows(2).map(|w| (w[1] / w[0]).ln()).collect();
    let mean = returns.iter().sum::<f64>() / returns.len() as f64;
    let variance = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>()
        / (returns.len() as f64 - 1.0).max(1.0);
    let std_dev = variance.sqrt();
    if std_dev < 1e-12 {
        return f64::NEG_INFINITY;
    }
    mean / std_dev * 252.0_f64.sqrt()
}

/// Compute Sortino ratio (downside deviation only) from a daily equity curve.
pub fn sortino_from_equity(equity: &[f64]) -> f64 {
    if equity.len() < 2 {
        return f64::NEG_INFINITY;
    }
    let returns: Vec<f64> = equity.windows(2).map(|w| (w[1] / w[0]).ln()).collect();
    let mean = returns.iter().sum::<f64>() / returns.len() as f64;
    let downside_variance = returns
        .iter()
        .filter(|&&r| r < 0.0)
        .map(|r| r.powi(2))
        .sum::<f64>()
        / (returns.len() as f64).max(1.0);
    let downside_std = downside_variance.sqrt();
    if downside_std < 1e-12 {
        return f64::NEG_INFINITY;
    }
    mean / downside_std * 252.0_f64.sqrt()
}

// ── EnsembleRunnerBuilder ─────────────────────────────────────────────────────

/// Fluent builder for `EnsembleRunner`.
#[derive(Default)]
pub struct EnsembleRunnerBuilder {
    stacks: Vec<(StrategyStack, f64)>,
    signal_threshold: Option<f64>,
}

impl EnsembleRunnerBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn stack(mut self, s: StrategyStack, weight: f64) -> Self {
        self.stacks.push((s, weight));
        self
    }

    pub fn signal_threshold(mut self, t: f64) -> Self {
        self.signal_threshold = Some(t);
        self
    }

    pub fn build(self) -> EnsembleRunner {
        EnsembleRunner::new(self.stacks, self.signal_threshold.unwrap_or(0.5))
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn avg_decimal_weighted<I: Iterator<Item = (f64, Decimal)>>(iter: I) -> Decimal {
    let pairs: Vec<(f64, Decimal)> = iter.collect();
    let total_w: f64 = pairs.iter().map(|(w, _)| *w).sum();
    if total_w <= 0.0 {
        return Decimal::ZERO;
    }
    let weighted_sum: f64 = pairs
        .iter()
        .filter_map(|(w, d)| d.to_string().parse::<f64>().ok().map(|v| w * v))
        .sum();
    Decimal::try_from(weighted_sum / total_w).unwrap_or(Decimal::ZERO)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sharpe_positive_trend() {
        // Steadily rising equity → positive Sharpe.
        let equity: Vec<f64> = (100..=200).map(|i| i as f64).collect();
        let s = sharpe_from_equity(&equity);
        assert!(s > 0.0, "Expected positive Sharpe, got {s}");
    }

    #[test]
    fn test_sharpe_insufficient_data() {
        assert_eq!(sharpe_from_equity(&[100.0]), f64::NEG_INFINITY);
        assert_eq!(sharpe_from_equity(&[]), f64::NEG_INFINITY);
    }

    #[test]
    fn test_project_simplex_already_valid() {
        let opt = WeightOptimizer::new(3);
        let v = vec![0.2, 0.5, 0.3];
        let projected = opt.project_simplex(&v);
        let sum: f64 = projected.iter().sum();
        assert!((sum - 1.0).abs() < 1e-9, "Sum should be 1.0, got {sum}");
        for &x in &projected {
            assert!(x >= 0.0, "All values should be non-negative");
        }
    }

    #[test]
    fn test_project_simplex_negative() {
        let opt = WeightOptimizer::new(3);
        let v = vec![-1.0, 2.0, 0.5];
        let projected = opt.project_simplex(&v);
        let sum: f64 = projected.iter().sum();
        assert!((sum - 1.0).abs() < 1e-9);
        for &x in &projected {
            assert!(x >= 0.0);
        }
    }

    #[test]
    fn test_optimiser_converges() {
        // Simple quadratic: maximum at [0.5, 0.5].
        let opt = WeightOptimizer::new(2).max_iterations(200).tolerance(1e-8);
        let result = opt.optimise(|w| -(w[0] - 0.5).powi(2) - (w[1] - 0.5).powi(2));
        // Best weights should be close to [0.5, 0.5].
        assert!(
            (result.weights[0] - 0.5).abs() < 0.05,
            "Weight[0]={:.4} not near 0.5",
            result.weights[0]
        );
    }
}
