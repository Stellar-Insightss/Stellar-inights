//! Streaming, mergeable percentile sketch based on DDSketch.
//!
//! # Mathematical Principles & Error Guarantees
//!
//! Exact quantile calculation over an unbounded, streaming dataset requires storing and sorting
//! every data point ($O(N)$ space and $O(N \log N)$ time), which is infeasible for high-throughput
//! financial ledger ingestion.
//!
//! DDSketch (Masson, Rim, Lim, VLDB 2019) solves this by mapping positive real values into
//! exponentially/geometrically sized buckets.
//!
//! Given a relative accuracy parameter $\alpha \in (0, 1)$ (default $\alpha = 0.01$, or 1% max relative error):
//! - The base of the geometric progression is $\gamma = \frac{1 + \alpha}{1 - \alpha}$.
//! - A positive value $v > 0$ is mapped to bucket index:
//!   $$k(v) = \left\lfloor \frac{\ln(v)}{\ln(\gamma)} \right\rfloor$$
//! - The representative value for bucket $k$ is its center:
//!   $$v_{\text{est}}(k) = \frac{2 \cdot \gamma^k}{1 + \gamma} = \gamma^k \cdot (1 - \alpha)$$
//!
//! ## Proven Error Bound
//! For any value $v \in [\gamma^k, \gamma^{k+1})$, the relative error of estimating $v$ with $v_{\text{est}}(k)$ is:
//! $$\text{Relative Error} = \frac{|v_{\text{est}}(k) - v|}{v} \le \alpha$$
//!
//! Consequently, for any quantile $q \in [0, 1]$, the estimated quantile $\hat{q}$ and the true quantile $q^*$ satisfy:
//! $$\frac{|\hat{q} - q^*|}{q^*} \le \alpha$$
//!
//! ## Mergeability
//! Two DDSketches $S_1$ and $S_2$ with the same $\alpha$ can be merged losslessly ($S = S_1 \oplus S_2$)
//! by simply summing the counts in corresponding buckets. Merging is commutative and associative,
//! enabling distributed aggregation across parallel ingestion shards and multi-resolution time windows.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

/// Default relative error bound $\alpha = 0.01$ (1% maximum relative error).
pub const DEFAULT_ALPHA: f64 = 0.01;

/// Minimum positive value distinguished from zero (1 nanosecond / 1e-6 ms).
pub const MIN_POSITIVE_VALUE: f64 = 1e-9;

#[derive(Debug, Error, PartialEq)]
pub enum SketchError {
    #[error("alpha must be in (0.0, 1.0), got {0}")]
    InvalidAlpha(f64),
    #[error("cannot merge sketches with differing alpha: {0} vs {1}")]
    AlphaMismatch(f64, f64),
    #[error("value must be non-negative, got {0}")]
    NegativeValue(f64),
    #[error("value is NaN or Infinite: {0}")]
    NonFiniteValue(f64),
}

/// Computed latency percentiles and summary statistics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PercentileSummary {
    pub count: u64,
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    pub sum: f64,
    pub p50: f64,
    pub p75: f64,
    pub p90: f64,
    pub p95: f64,
    pub p99: f64,
    pub p999: f64,
}

/// A mergeable, bounded-error streaming quantile sketch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DDSketch {
    alpha: f64,
    gamma: f64,
    ln_gamma: f64,
    count: u64,
    zero_count: u64,
    sum: f64,
    min: f64,
    max: f64,
    /// Mapping from bucket index $k$ to sample count.
    buckets: BTreeMap<i32, u64>,
}

impl PartialEq for DDSketch {
    fn eq(&self, other: &Self) -> bool {
        (self.alpha - other.alpha).abs() < 1e-9
            && self.count == other.count
            && self.zero_count == other.zero_count
            && (self.sum - other.sum).abs() < 1e-6
            && (self.min - other.min).abs() < 1e-6
            && (self.max - other.max).abs() < 1e-6
            && self.buckets == other.buckets
    }
}

impl Default for DDSketch {
    fn default() -> Self {
        Self::new(DEFAULT_ALPHA).expect("DEFAULT_ALPHA is valid")
    }
}

impl DDSketch {
    /// Creates a new DDSketch with the specified relative error parameter $\alpha \in (0, 1)$.
    pub fn new(alpha: f64) -> Result<Self, SketchError> {
        if alpha <= 0.0 || alpha >= 1.0 || alpha.is_nan() {
            return Err(SketchError::InvalidAlpha(alpha));
        }

        let gamma = (1.0 + alpha) / (1.0 - alpha);
        let ln_gamma = gamma.ln();

        Ok(Self {
            alpha,
            gamma,
            ln_gamma,
            count: 0,
            zero_count: 0,
            sum: 0.0,
            min: f64::INFINITY,
            max: f64::NEG_INFINITY,
            buckets: BTreeMap::new(),
        })
    }

    /// Returns the configured relative error parameter $\alpha$.
    pub fn alpha(&self) -> f64 {
        self.alpha
    }

    /// Returns the total number of inserted samples.
    pub fn count(&self) -> u64 {
        self.count
    }

    /// Returns the sum of all inserted samples.
    pub fn sum(&self) -> f64 {
        self.sum
    }

    /// Returns the minimum value inserted, or `None` if empty.
    pub fn min(&self) -> Option<f64> {
        if self.count == 0 {
            None
        } else {
            Some(self.min)
        }
    }

    /// Returns the maximum value inserted, or `None` if empty.
    pub fn max(&self) -> Option<f64> {
        if self.count == 0 {
            None
        } else {
            Some(self.max)
        }
    }

    /// Returns the arithmetic mean of all inserted samples, or `0.0` if empty.
    pub fn mean(&self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            self.sum / (self.count as f64)
        }
    }

    /// Returns whether the sketch is empty.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Returns the number of active buckets.
    pub fn num_buckets(&self) -> usize {
        self.buckets.len()
    }

    /// Computes the bucket index for a given positive value.
    fn key_for_value(&self, value: f64) -> i32 {
        (value.ln() / self.ln_gamma).floor() as i32
    }

    /// Computes the representative estimated value for a given bucket index $k$.
    fn value_for_key(&self, key: i32) -> f64 {
        // The bucket covers [gamma^k, gamma^(k+1)).
        // Representative value is 2 * gamma^(k+1) / (1 + gamma) = gamma^k * 2 * gamma / (1 + gamma)
        let gamma_k = self.gamma.powi(key);
        (2.0 * gamma_k * self.gamma) / (1.0 + self.gamma)
    }

    /// Inserts a single non-negative value into the sketch.
    pub fn add(&mut self, value: f64) -> Result<(), SketchError> {
        if value.is_nan() || value.is_infinite() {
            return Err(SketchError::NonFiniteValue(value));
        }
        if value < 0.0 {
            return Err(SketchError::NegativeValue(value));
        }

        self.count += 1;
        self.sum += value;
        if value < self.min {
            self.min = value;
        }
        if value > self.max {
            self.max = value;
        }

        if value <= MIN_POSITIVE_VALUE {
            self.zero_count += 1;
        } else {
            let key = self.key_for_value(value);
            *self.buckets.entry(key).or_insert(0) += 1;
        }

        Ok(())
    }

    /// Inserts a value with a given count weight.
    pub fn add_weighted(&mut self, value: f64, weight: u64) -> Result<(), SketchError> {
        if weight == 0 {
            return Ok(());
        }
        if value.is_nan() || value.is_infinite() {
            return Err(SketchError::NonFiniteValue(value));
        }
        if value < 0.0 {
            return Err(SketchError::NegativeValue(value));
        }

        self.count += weight;
        self.sum += value * (weight as f64);
        if value < self.min {
            self.min = value;
        }
        if value > self.max {
            self.max = value;
        }

        if value <= MIN_POSITIVE_VALUE {
            self.zero_count += weight;
        } else {
            let key = self.key_for_value(value);
            *self.buckets.entry(key).or_insert(0) += weight;
        }

        Ok(())
    }

    /// Merges another DDSketch into this sketch.
    ///
    /// This operation is exact and lossless: $S_1 \oplus S_2$.
    pub fn merge(&mut self, other: &Self) -> Result<(), SketchError> {
        if (self.alpha - other.alpha).abs() > 1e-9 {
            return Err(SketchError::AlphaMismatch(self.alpha, other.alpha));
        }

        if other.count == 0 {
            return Ok(());
        }

        if self.count == 0 {
            self.count = other.count;
            self.zero_count = other.zero_count;
            self.sum = other.sum;
            self.min = other.min;
            self.max = other.max;
            self.buckets = other.buckets.clone();
            return Ok(());
        }

        self.count += other.count;
        self.zero_count += other.zero_count;
        self.sum += other.sum;
        if other.min < self.min {
            self.min = other.min;
        }
        if other.max > self.max {
            self.max = other.max;
        }

        for (&key, &cnt) in &other.buckets {
            *self.buckets.entry(key).or_insert(0) += cnt;
        }

        Ok(())
    }

    /// Queries the estimate for a specific quantile $q \in [0.0, 1.0]$.
    ///
    /// Returns `None` if the sketch has no data.
    pub fn quantile(&self, q: f64) -> Option<f64> {
        if self.count == 0 || q.is_nan() {
            return None;
        }

        let q_clamped = q.clamp(0.0, 1.0);

        // Edge cases
        if q_clamped == 0.0 {
            return Some(self.min);
        }
        if (q_clamped - 1.0).abs() < 1e-9 {
            return Some(self.max);
        }

        // Target rank in 1-based indexing
        let rank = (q_clamped * (self.count as f64)).ceil() as u64;
        let target_rank = rank.max(1);

        if target_rank <= self.zero_count {
            return Some(0.0);
        }

        let mut cumulative = self.zero_count;
        for (&key, &cnt) in &self.buckets {
            cumulative += cnt;
            if cumulative >= target_rank {
                let val = self.value_for_key(key);
                // Clamp within observed min/max
                return Some(val.clamp(self.min, self.max));
            }
        }

        Some(self.max)
    }

    /// Computes a standard percentile summary (P50, P75, P90, P95, P99, P99.9, min, max, mean).
    pub fn summary(&self) -> Option<PercentileSummary> {
        if self.count == 0 {
            return None;
        }

        Some(PercentileSummary {
            count: self.count,
            min: self.min,
            max: self.max,
            mean: self.mean(),
            sum: self.sum,
            p50: self.quantile(0.50).unwrap_or(0.0),
            p75: self.quantile(0.75).unwrap_or(0.0),
            p90: self.quantile(0.90).unwrap_or(0.0),
            p95: self.quantile(0.95).unwrap_or(0.0),
            p99: self.quantile(0.99).unwrap_or(0.0),
            p999: self.quantile(0.999).unwrap_or(0.0),
        })
    }

    /// Returns a deterministic digest representation of the sketch state for hashing/reconciliation.
    pub fn bucket_entries(&self) -> Vec<(i32, u64)> {
        self.buckets.iter().map(|(&k, &v)| (k, v)).collect()
    }
}

/// Exact percentile calculator for small datasets, ground truth benchmarking, and error bound testing.
#[derive(Debug, Clone, Default)]
pub struct ExactSummary {
    values: Vec<f64>,
}

impl ExactSummary {
    pub fn new() -> Self {
        Self { values: Vec::new() }
    }

    pub fn add(&mut self, value: f64) {
        self.values.push(value);
    }

    pub fn count(&self) -> usize {
        self.values.len()
    }

    pub fn quantile(&self, q: f64) -> Option<f64> {
        if self.values.is_empty() || q.is_nan() {
            return None;
        }

        let mut sorted = self.values.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let q_clamped = q.clamp(0.0, 1.0);
        if q_clamped == 0.0 {
            return Some(sorted[0]);
        }
        if (q_clamped - 1.0).abs() < 1e-9 {
            return Some(sorted[sorted.len() - 1]);
        }

        let rank = (q_clamped * ((sorted.len() - 1) as f64)).round() as usize;
        Some(sorted[rank])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ddsketch_empty() {
        let sketch = DDSketch::default();
        assert_eq!(sketch.count(), 0);
        assert_eq!(sketch.min(), None);
        assert_eq!(sketch.max(), None);
        assert_eq!(sketch.quantile(0.5), None);
        assert_eq!(sketch.summary(), None);
    }

    #[test]
    fn test_ddsketch_single_value() {
        let mut sketch = DDSketch::new(0.01).unwrap();
        sketch.add(100.0).unwrap();

        assert_eq!(sketch.count(), 1);
        assert_eq!(sketch.min(), Some(100.0));
        assert_eq!(sketch.max(), Some(100.0));
        assert_eq!(sketch.mean(), 100.0);

        let p50 = sketch.quantile(0.5).unwrap();
        let relative_error = (p50 - 100.0).abs() / 100.0;
        assert!(
            relative_error <= 0.01,
            "Error {} exceeded alpha 0.01",
            relative_error
        );
    }

    #[test]
    fn test_ddsketch_error_bound_across_quantiles() {
        let alpha = 0.01; // 1% relative error bound
        let mut sketch = DDSketch::new(alpha).unwrap();
        let mut exact = ExactSummary::new();

        // Populate with synthetic latency data (log-normal distribution)
        // Latencies ranging from 10ms to 15,000ms
        for i in 1..=5000 {
            let val = ((i as f64) * 0.37).sin().abs() * 2000.0 + (i as f64) * 1.5 + 5.0;
            sketch.add(val).unwrap();
            exact.add(val);
        }

        let quantiles = [0.10, 0.25, 0.50, 0.75, 0.90, 0.95, 0.99, 0.999];

        for &q in &quantiles {
            let true_q = exact.quantile(q).unwrap();
            let est_q = sketch.quantile(q).unwrap();
            let rel_err = (est_q - true_q).abs() / true_q;

            assert!(
                rel_err <= alpha + 1e-4,
                "At quantile q={}: true={}, est={}, rel_err={} > alpha={}",
                q,
                true_q,
                est_q,
                rel_err,
                alpha
            );
        }
    }

    #[test]
    fn test_ddsketch_mergeability() {
        let mut shard1 = DDSketch::new(0.01).unwrap();
        let mut shard2 = DDSketch::new(0.01).unwrap();
        let mut combined = DDSketch::new(0.01).unwrap();

        for i in 1..=1000 {
            let v1 = (i as f64) * 2.5;
            let v2 = (i as f64) * 3.7 + 10.0;
            shard1.add(v1).unwrap();
            shard2.add(v2).unwrap();
            combined.add(v1).unwrap();
            combined.add(v2).unwrap();
        }

        shard1.merge(&shard2).unwrap();

        assert_eq!(shard1.count(), combined.count());
        assert_eq!(shard1.min(), combined.min());
        assert_eq!(shard1.max(), combined.max());
        assert!((shard1.sum() - combined.sum()).abs() < 1e-6);

        for &q in &[0.50, 0.90, 0.95, 0.99] {
            let q_merged = shard1.quantile(q).unwrap();
            let q_combined = combined.quantile(q).unwrap();
            assert!(
                (q_merged - q_combined).abs() < 1e-6,
                "Mismatch at q={}: merged={}, combined={}",
                q,
                q_merged,
                q_combined
            );
        }
    }

    #[test]
    fn test_zero_and_edge_values() {
        let mut sketch = DDSketch::default();
        sketch.add(0.0).unwrap();
        sketch.add(0.0).unwrap();
        sketch.add(10.0).unwrap();

        assert_eq!(sketch.count(), 3);
        assert_eq!(sketch.min(), Some(0.0));
        assert_eq!(sketch.quantile(0.0), Some(0.0));
        assert_eq!(sketch.quantile(0.5), Some(0.0));
        assert!(sketch.quantile(0.99).unwrap() > 0.0);
    }
}
