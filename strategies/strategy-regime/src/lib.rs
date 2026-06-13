//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! `strategy-regime` — Identifier plugin (§8.C.3)
//!
//! A Hidden Markov Model (HMM) based market regime classifier.
//!
//! ## How it works
//!
//! For each symbol in the universe the plugin extracts a 3-dimensional
//! observation sequence from daily bar history:
//!
//! 1. **Rolling return** — 20-day total return (momentum proxy).
//! 2. **Rolling volatility** — 20-day annualised standard deviation of log returns.
//! 3. **Volume ratio** — current volume / 20-day average volume (activity proxy).
//!
//! Each observation is discretised into a small alphabet (bins) and fed through a
//! Gaussian HMM with `n_states` hidden states (default 4).  The model is trained
//! once per run using the Baum-Welch EM algorithm and the Viterbi algorithm then
//! decodes the most likely state sequence, producing a regime label for the
//! *current* bar (the last observation).
//!
//! ## Regime labels
//!
//! States are labelled post-hoc by inspecting the decoded sequence:
//!
//! | State characteristics       | Label            |
//! |-----------------------------|------------------|
//! | High return, low vol        | `trending-up`    |
//! | Low/neg return, low vol     | `trending-down`  |
//! | Low return, low vol         | `ranging`        |
//! | Any return, high vol        | `high-volatility`|
//!
//! Candidates are passed through only when the current regime is `trending-up`
//! or `ranging` (configurable via `favorable_regimes`).
//!
//! ## HMM implementation
//!
//! A full Gaussian HMM (diagonal covariance) is implemented from scratch using:
//! - **Forward-backward** algorithm (Baum-Welch E-step)
//! - **M-step** updates for transition matrix, emission means/variances, and initial priors
//! - **Viterbi** decoding for the final regime sequence
//!
//! No external ML crates are required.
//!
//! # Parameters (read from `ctx.parameters`)
//!
//! | Key                 | Default                            | Description                         |
//! |---------------------|------------------------------------|-------------------------------------|
//! | `n_states`          | `4`                                | Number of hidden states             |
//! | `lookback_days`     | `252`                              | Bar history window for training     |
//! | `feature_window`    | `20`                               | Rolling window for feature computation |
//! | `max_iter`          | `50`                               | Max Baum-Welch EM iterations        |
//! | `tol`               | `1e-4`                             | EM convergence tolerance            |
//! | `favorable_regimes` | `trending-up,ranging`              | Comma-separated list of pass-through regimes |

use async_trait::async_trait;
use economind_core::model::DailyCandleEntry;
use economind_strategy::{Candidate, Identifier, StrategyContext};
use rust_decimal::prelude::*;
use std::collections::HashMap;

// ── RegimeIdentifier ──────────────────────────────────────────────────────────

pub struct RegimeIdentifier {
    n_states: usize,
    lookback_days: usize,
    feature_window: usize,
    max_iter: usize,
    tol: f64,
    favorable_regimes: Vec<String>,
}

impl RegimeIdentifier {
    pub fn new(parameters: &HashMap<String, String>) -> Self {
        let favorable = parameters
            .get("favorable_regimes")
            .map(|s| {
                s.split(',')
                    .map(|r| r.trim().to_string())
                    .filter(|r| !r.is_empty())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| vec!["trending-up".to_string(), "ranging".to_string()]);

        Self {
            n_states: parameters
                .get("n_states")
                .and_then(|v| v.parse().ok())
                .unwrap_or(4),
            lookback_days: parameters
                .get("lookback_days")
                .and_then(|v| v.parse().ok())
                .unwrap_or(252),
            feature_window: parameters
                .get("feature_window")
                .and_then(|v| v.parse().ok())
                .unwrap_or(20),
            max_iter: parameters
                .get("max_iter")
                .and_then(|v| v.parse().ok())
                .unwrap_or(50),
            tol: parameters
                .get("tol")
                .and_then(|v| v.parse().ok())
                .unwrap_or(1e-4),
            favorable_regimes: favorable,
        }
    }

    /// Classify the current regime for a symbol's bar history.
    ///
    /// Returns the regime label for the most recent bar, or `None` if the
    /// history is too short.
    pub fn classify(&self, bars: &[DailyCandleEntry]) -> Option<String> {
        let n = bars.len();
        let min_bars = self.feature_window + self.n_states * 2 + 1;
        if n < min_bars {
            return None;
        }

        // Use the most recent `lookback_days` bars.
        let window = &bars[n.saturating_sub(self.lookback_days)..];

        // Extract 3-D observation sequence.
        let obs = extract_features(window, self.feature_window);
        if obs.len() < self.n_states * 2 {
            return None;
        }

        // Train HMM via Baum-Welch.
        let mut hmm = GaussianHmm::init(self.n_states, &obs);
        hmm.fit(&obs, self.max_iter, self.tol);

        // Decode current regime via Viterbi.
        let states = hmm.viterbi(&obs);
        let current_state = *states.last()?;

        // Label the state by inspecting emission parameters.
        let label = hmm.label_state(current_state);
        Some(label)
    }
}

#[async_trait]
impl Identifier for RegimeIdentifier {
    fn name(&self) -> &str {
        "regime"
    }

    async fn identify(&self, ctx: &StrategyContext) -> Vec<Candidate> {
        let mut candidates: Vec<Candidate> = Vec::new();

        for symbol in &ctx.universe {
            let bars = match ctx.bars.get(symbol) {
                Some(b) => b,
                None => continue,
            };

            let regime = match self.classify(bars) {
                Some(r) => r,
                None => {
                    // Not enough data to classify — skip conservatively.
                    continue;
                }
            };

            // Only pass candidates in favorable regimes.
            if !self.favorable_regimes.contains(&regime) {
                continue;
            }

            // Score: 1.0 for trending-up (strongest), 0.7 for ranging (weaker).
            let score = if regime == "trending-up" { 1.0 } else { 0.7 };

            let mut metadata = HashMap::new();
            metadata.insert("regime".to_string(), regime);

            candidates.push(Candidate {
                symbol: symbol.clone(),
                score,
                metadata,
            });
        }

        // Sort descending by score.
        candidates.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        candidates
    }
}

// ── Feature extraction ────────────────────────────────────────────────────────

/// Observation type: [rolling_return, rolling_vol, volume_ratio].
type Obs = [f64; 3];

/// Extract a sequence of 3-D observations from bar history.
///
/// Feature window `w` determines the rolling lookback for each feature.
/// Output length = `bars.len() - w`.
fn extract_features(bars: &[DailyCandleEntry], w: usize) -> Vec<Obs> {
    let n = bars.len();
    if n < w + 1 {
        return vec![];
    }

    // Average volume over entire window for the ratio denominator.
    let avg_vol: f64 = bars.iter().map(|b| b.volume as f64).sum::<f64>() / n as f64;

    let mut obs = Vec::with_capacity(n - w);

    for i in w..n {
        let window = &bars[i - w..=i];

        // 1. Rolling return: (close[i] / close[i-w]) - 1
        let close_start = window.first().unwrap().close.to_f64().unwrap_or(1.0);
        let close_end = window.last().unwrap().close.to_f64().unwrap_or(1.0);
        let rolling_return = if close_start > 0.0 {
            close_end / close_start - 1.0
        } else {
            0.0
        };

        // 2. Rolling volatility: annualised std dev of daily log returns.
        let log_rets: Vec<f64> = window
            .windows(2)
            .filter_map(|pair| {
                let prev = pair[0].close.to_f64()?;
                let curr = pair[1].close.to_f64()?;
                if prev > 0.0 { Some((curr / prev).ln()) } else { None }
            })
            .collect();
        let vol = annualised_vol(&log_rets);

        // 3. Volume ratio: current bar volume / rolling average volume.
        let current_vol = window.last().unwrap().volume as f64;
        let window_avg_vol: f64 =
            window.iter().map(|b| b.volume as f64).sum::<f64>() / window.len() as f64;
        let vol_ratio = if window_avg_vol > 0.0 {
            current_vol / window_avg_vol
        } else {
            1.0
        };

        obs.push([rolling_return, vol, vol_ratio]);
    }

    obs
}

fn annualised_vol(log_rets: &[f64]) -> f64 {
    if log_rets.len() < 2 {
        return 0.0;
    }
    let mean = log_rets.iter().sum::<f64>() / log_rets.len() as f64;
    let variance = log_rets
        .iter()
        .map(|r| (r - mean).powi(2))
        .sum::<f64>()
        / (log_rets.len() as f64 - 1.0).max(1.0);
    variance.sqrt() * 252.0_f64.sqrt()
}

// ── Gaussian HMM ─────────────────────────────────────────────────────────────

/// A Gaussian HMM with diagonal covariance, trained via Baum-Welch.
///
/// Dimensions: `n_states` hidden states, `D=3` observation dimensions.
struct GaussianHmm {
    n_states: usize,
    /// Initial state distribution π (log space).
    log_pi: Vec<f64>,
    /// Transition matrix A[i][j] = P(s_t=j | s_{t-1}=i) (log space).
    log_a: Vec<Vec<f64>>,
    /// Emission means: means[k][d].
    means: Vec<Vec<f64>>,
    /// Emission variances (diagonal): vars[k][d].
    vars: Vec<Vec<f64>>,
}

const D: usize = 3; // Observation dimensionality.
const LOG_TINY: f64 = -1e30; // Substitute for -∞ in log space.

impl GaussianHmm {
    /// Initialise with k-means style spread over the observation range.
    fn init(n_states: usize, obs: &[Obs]) -> Self {
        let k = n_states;
        // Compute global mean and std per dimension for sensible init.
        let mut global_mean = [0.0_f64; D];
        let mut global_var = [1.0_f64; D];
        let n = obs.len() as f64;

        for d in 0..D {
            global_mean[d] = obs.iter().map(|o| o[d]).sum::<f64>() / n;
        }
        for d in 0..D {
            let m = global_mean[d];
            global_var[d] =
                (obs.iter().map(|o| (o[d] - m).powi(2)).sum::<f64>() / n).max(1e-6);
        }

        // Space initial means evenly across ±1 std dev.
        let mut means: Vec<Vec<f64>> = Vec::with_capacity(k);
        for i in 0..k {
            let t = if k == 1 {
                0.0
            } else {
                -1.0 + 2.0 * i as f64 / (k - 1) as f64
            };
            let m: Vec<f64> = (0..D)
                .map(|d| global_mean[d] + t * global_var[d].sqrt())
                .collect();
            means.push(m);
        }

        // Initial variances = global variances.
        let vars: Vec<Vec<f64>> = vec![global_var.to_vec(); k];

        // Uniform transition and initial distribution.
        let log_uniform = (1.0 / k as f64).ln();
        let log_pi = vec![log_uniform; k];
        let log_a = vec![vec![log_uniform; k]; k];

        Self { n_states: k, log_pi, log_a, means, vars }
    }

    // ── Log emission probability ──────────────────────────────────────────────

    /// Log probability of observation `o` under state `k` (diagonal Gaussian).
    fn log_emit(&self, k: usize, o: &Obs) -> f64 {
        let mut lp = 0.0_f64;
        for d in 0..D {
            let v = self.vars[k][d].max(1e-10);
            let diff = o[d] - self.means[k][d];
            lp -= 0.5 * (diff * diff / v + v.ln() + (2.0 * std::f64::consts::PI).ln());
        }
        lp
    }

    // ── Forward algorithm (log scale) ────────────────────────────────────────

    fn forward(&self, obs: &[Obs]) -> Vec<Vec<f64>> {
        let t = obs.len();
        let k = self.n_states;
        let mut alpha = vec![vec![LOG_TINY; k]; t];

        for j in 0..k {
            alpha[0][j] = self.log_pi[j] + self.log_emit(j, &obs[0]);
        }

        for t_i in 1..t {
            for j in 0..k {
                let log_sum = log_sum_exp_vec(
                    (0..k).map(|i| alpha[t_i - 1][i] + self.log_a[i][j]).collect(),
                );
                alpha[t_i][j] = log_sum + self.log_emit(j, &obs[t_i]);
            }
        }

        alpha
    }

    // ── Backward algorithm (log scale) ───────────────────────────────────────

    fn backward(&self, obs: &[Obs]) -> Vec<Vec<f64>> {
        let t = obs.len();
        let k = self.n_states;
        let mut beta = vec![vec![0.0_f64; k]; t]; // log(1) = 0

        for t_i in (0..t - 1).rev() {
            for i in 0..k {
                beta[t_i][i] = log_sum_exp_vec(
                    (0..k)
                        .map(|j| {
                            self.log_a[i][j]
                                + self.log_emit(j, &obs[t_i + 1])
                                + beta[t_i + 1][j]
                        })
                        .collect(),
                );
            }
        }

        beta
    }

    // ── Baum-Welch EM ─────────────────────────────────────────────────────────

    fn fit(&mut self, obs: &[Obs], max_iter: usize, tol: f64) {
        let t = obs.len();
        let k = self.n_states;
        let mut prev_ll = f64::NEG_INFINITY;

        for _iter in 0..max_iter {
            let alpha = self.forward(obs);
            let beta = self.backward(obs);

            // Log-likelihood.
            let ll = log_sum_exp_vec(alpha[t - 1].clone());
            if (ll - prev_ll).abs() < tol {
                break;
            }
            prev_ll = ll;

            // ── E-step: compute γ and ξ ───────────────────────────────────────

            // γ[t][k] = P(s_t = k | obs, model) in log space.
            let mut log_gamma: Vec<Vec<f64>> = vec![vec![LOG_TINY; k]; t];
            for t_i in 0..t {
                let log_denom = log_sum_exp_vec(
                    (0..k).map(|j| alpha[t_i][j] + beta[t_i][j]).collect(),
                );
                for j in 0..k {
                    log_gamma[t_i][j] = alpha[t_i][j] + beta[t_i][j] - log_denom;
                }
            }

            // ξ[t][i][j] = P(s_t=i, s_{t+1}=j | obs, model) in log space.
            let mut log_xi: Vec<Vec<Vec<f64>>> =
                vec![vec![vec![LOG_TINY; k]; k]; t.saturating_sub(1)];
            for t_i in 0..t.saturating_sub(1) {
                let log_denom = log_sum_exp_vec(
                    (0..k)
                        .flat_map(|i| {
                            (0..k).map(move |j| {
                                alpha[t_i][i]
                                    + self.log_a[i][j]
                                    + self.log_emit(j, &obs[t_i + 1])
                                    + beta[t_i + 1][j]
                            })
                        })
                        .collect(),
                );
                for i in 0..k {
                    for j in 0..k {
                        log_xi[t_i][i][j] = alpha[t_i][i]
                            + self.log_a[i][j]
                            + self.log_emit(j, &obs[t_i + 1])
                            + beta[t_i + 1][j]
                            - log_denom;
                    }
                }
            }

            // ── M-step ───────────────────────────────────────────────────────

            // Update π.
            for j in 0..k {
                self.log_pi[j] = log_gamma[0][j];
            }
            // Normalise π.
            let log_sum_pi = log_sum_exp_vec(self.log_pi.clone());
            for j in 0..k {
                self.log_pi[j] -= log_sum_pi;
            }

            // Update A.
            for i in 0..k {
                let log_denom_i = log_sum_exp_vec(
                    (0..t.saturating_sub(1))
                        .map(|t_i| log_gamma[t_i][i])
                        .collect(),
                );
                for j in 0..k {
                    let log_numer = log_sum_exp_vec(
                        (0..t.saturating_sub(1))
                            .map(|t_i| log_xi[t_i][i][j])
                            .collect(),
                    );
                    self.log_a[i][j] = log_numer - log_denom_i;
                }
            }

            // Update emission means and variances.
            for j in 0..k {
                let log_denom_j =
                    log_sum_exp_vec((0..t).map(|t_i| log_gamma[t_i][j]).collect());
                let gamma_sum = log_denom_j.exp().max(1e-30);

                for d in 0..D {
                    // Mean update.
                    let new_mean: f64 = (0..t)
                        .map(|t_i| log_gamma[t_i][j].exp() * obs[t_i][d])
                        .sum::<f64>()
                        / gamma_sum;
                    self.means[j][d] = new_mean;

                    // Variance update.
                    let new_var: f64 = (0..t)
                        .map(|t_i| {
                            log_gamma[t_i][j].exp() * (obs[t_i][d] - new_mean).powi(2)
                        })
                        .sum::<f64>()
                        / gamma_sum;
                    self.vars[j][d] = new_var.max(1e-6);
                }
            }
        }
    }

    // ── Viterbi decoding ──────────────────────────────────────────────────────

    fn viterbi(&self, obs: &[Obs]) -> Vec<usize> {
        let t = obs.len();
        let k = self.n_states;

        let mut delta = vec![vec![LOG_TINY; k]; t];
        let mut psi = vec![vec![0usize; k]; t];

        // Initialise.
        for j in 0..k {
            delta[0][j] = self.log_pi[j] + self.log_emit(j, &obs[0]);
        }

        // Recursion.
        for t_i in 1..t {
            for j in 0..k {
                let (best_prev, best_val) = (0..k)
                    .map(|i| (i, delta[t_i - 1][i] + self.log_a[i][j]))
                    .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
                    .unwrap_or((0, LOG_TINY));
                psi[t_i][j] = best_prev;
                delta[t_i][j] = best_val + self.log_emit(j, &obs[t_i]);
            }
        }

        // Backtrack.
        let mut states = vec![0usize; t];
        states[t - 1] = (0..k)
            .max_by(|&a, &b| {
                delta[t - 1][a]
                    .partial_cmp(&delta[t - 1][b])
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap_or(0);
        for t_i in (0..t - 1).rev() {
            states[t_i] = psi[t_i + 1][states[t_i + 1]];
        }

        states
    }

    // ── Regime labelling ──────────────────────────────────────────────────────

    /// Assign a human-readable regime label to a state by examining its
    /// emission mean vector.
    ///
    /// Features: [rolling_return, rolling_vol, volume_ratio]
    /// - High vol (dim 1 > 0.30 annualised) → "high-volatility"
    /// - High return (dim 0 > 0.05), low vol → "trending-up"
    /// - Low/neg return (dim 0 < -0.02), low vol → "trending-down"
    /// - Otherwise → "ranging"
    fn label_state(&self, state: usize) -> String {
        let ret = self.means[state][0];
        let vol = self.means[state][1];

        if vol > 0.30 {
            "high-volatility".to_string()
        } else if ret > 0.05 {
            "trending-up".to_string()
        } else if ret < -0.02 {
            "trending-down".to_string()
        } else {
            "ranging".to_string()
        }
    }
}

// ── Numerics helpers ──────────────────────────────────────────────────────────

/// Log-sum-exp for a vector of log-probabilities.
///
/// Numerically stable: `log(Σ exp(xᵢ)) = max + log(Σ exp(xᵢ − max))`.
fn log_sum_exp_vec(logs: Vec<f64>) -> f64 {
    if logs.is_empty() {
        return LOG_TINY;
    }
    let max = logs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    if max == f64::NEG_INFINITY {
        return LOG_TINY;
    }
    let sum: f64 = logs.iter().map(|&x| (x - max).exp()).sum();
    max + sum.ln()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_bars(prices: &[f64], volumes: &[u64]) -> Vec<DailyCandleEntry> {
        prices
            .iter()
            .zip(volumes.iter())
            .enumerate()
            .map(|(i, (&p, &v))| {
                let d = rust_decimal::Decimal::try_from(p).unwrap();
                DailyCandleEntry {
                    date: chrono::NaiveDate::from_ymd_opt(2023, 1, 1).unwrap()
                        + chrono::Duration::days(i as i64),
                    open: d,
                    high: d * rust_decimal::Decimal::try_from(1.005).unwrap(),
                    low: d * rust_decimal::Decimal::try_from(0.995).unwrap(),
                    close: d,
                    volume: v,
                }
            })
            .collect()
    }

    #[test]
    fn test_log_sum_exp_basic() {
        let logs = vec![0.0_f64, 0.0, 0.0]; // e^0 + e^0 + e^0 = 3 → ln(3)
        let result = log_sum_exp_vec(logs);
        assert!((result - 3.0_f64.ln()).abs() < 1e-9, "Got {result}");
    }

    #[test]
    fn test_feature_extraction_length() {
        let prices: Vec<f64> = (100..=160).map(|i| i as f64).collect();
        let volumes: Vec<u64> = vec![1_000_000; prices.len()];
        let bars = synthetic_bars(&prices, &volumes);
        let obs = extract_features(&bars, 20);
        assert_eq!(obs.len(), bars.len() - 20, "Feature length mismatch");
    }

    #[test]
    fn test_hmm_init_and_viterbi() {
        // Trending-up market: steadily rising prices.
        let prices: Vec<f64> = (100..=200).map(|i| i as f64).collect();
        let volumes: Vec<u64> = vec![1_000_000; prices.len()];
        let bars = synthetic_bars(&prices, &volumes);
        let obs = extract_features(&bars, 20);

        assert!(!obs.is_empty(), "No observations extracted");

        let mut hmm = GaussianHmm::init(4, &obs);
        hmm.fit(&obs, 30, 1e-4);
        let states = hmm.viterbi(&obs);

        assert_eq!(states.len(), obs.len(), "Viterbi length mismatch");
        // All states should be in [0, 3].
        for &s in &states {
            assert!(s < 4, "Invalid state: {s}");
        }
    }

    #[test]
    fn test_classify_returns_label() {
        let identifier = RegimeIdentifier::new(&HashMap::new());

        // 300 bars of trending-up market.
        let prices: Vec<f64> = (100..=400).map(|i| i as f64).collect();
        let volumes: Vec<u64> = vec![1_000_000; prices.len()];
        let bars = synthetic_bars(&prices, &volumes);

        let label = identifier.classify(&bars);
        assert!(label.is_some(), "Expected a regime label");
        // The label should be one of the four valid regimes.
        let valid = ["trending-up", "trending-down", "ranging", "high-volatility"];
        assert!(
            valid.contains(&label.unwrap().as_str()),
            "Unexpected regime label"
        );
    }

    #[test]
    fn test_classify_insufficient_data() {
        let identifier = RegimeIdentifier::new(&HashMap::new());
        let bars = synthetic_bars(&[100.0, 101.0, 102.0], &[1_000; 3]);
        assert!(identifier.classify(&bars).is_none());
    }

    #[test]
    fn test_annualised_vol_constant() {
        // Zero log returns → zero vol.
        let returns = vec![0.0_f64; 20];
        assert!(annualised_vol(&returns) < 1e-10);
    }
}
