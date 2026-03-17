// nea-analysis: Statistical and numerical analysis for destruction-price correlation.

use chrono::NaiveDate;
use serde::Serialize;
use tracing::debug;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct LagCorrelation {
    pub lag: i32,
    pub correlation: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct GrangerResult {
    pub f_statistic: f64,
    pub p_value: f64,
    pub significant: bool,
    pub lags_used: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct AnalysisResult {
    pub optimal_lag: LagCorrelation,
    pub ccf: Vec<LagCorrelation>,
    pub granger: GrangerResult,
    pub confidence_threshold: f64,
}

// ---------------------------------------------------------------------------
// Module: timeseries
// ---------------------------------------------------------------------------

pub mod timeseries {
    use chrono::NaiveDate;
    use std::collections::BTreeMap;

    /// Align two time series to the same daily timestamps.
    /// Forward-fill prices for missing days, zero-fill destruction for missing days.
    pub fn align_series(
        destruction: &[(NaiveDate, f64)],
        prices: &[(NaiveDate, f64)],
    ) -> (Vec<f64>, Vec<f64>) {
        // Build maps
        let dest_map: BTreeMap<NaiveDate, f64> =
            destruction.iter().cloned().collect();
        let price_map: BTreeMap<NaiveDate, f64> =
            prices.iter().cloned().collect();

        // Find the overlapping date range
        let min_date = *dest_map
            .keys()
            .next()
            .unwrap_or(&NaiveDate::MIN)
            .max(price_map.keys().next().unwrap_or(&NaiveDate::MIN));
        let max_date = *dest_map
            .keys()
            .next_back()
            .unwrap_or(&NaiveDate::MIN)
            .min(price_map.keys().next_back().unwrap_or(&NaiveDate::MIN));

        if min_date > max_date {
            return (vec![], vec![]);
        }

        let mut aligned_dest = Vec::new();
        let mut aligned_price = Vec::new();
        let mut last_price: Option<f64> = None;

        // Find the most recent price on or before min_date for forward-fill seed
        for (&d, &v) in price_map.iter() {
            if d <= min_date {
                last_price = Some(v);
            } else {
                break;
            }
        }

        let mut current = min_date;
        while current <= max_date {
            // Destruction: zero-fill missing
            let d_val = dest_map.get(&current).copied().unwrap_or(0.0);
            aligned_dest.push(d_val);

            // Prices: forward-fill missing
            if let Some(&p) = price_map.get(&current) {
                last_price = Some(p);
            }
            aligned_price.push(last_price.unwrap_or(0.0));

            current = current
                .succ_opt()
                .expect("date overflow");
        }

        (aligned_dest, aligned_price)
    }

    /// First-order differencing for stationarity.
    pub fn difference(series: &[f64]) -> Vec<f64> {
        series
            .windows(2)
            .map(|w| w[1] - w[0])
            .collect()
    }

    /// Z-score normalization. Returns zeros if std_dev is 0.
    pub fn z_normalize(series: &[f64]) -> Vec<f64> {
        if series.is_empty() {
            return vec![];
        }
        let n = series.len() as f64;
        let mean = series.iter().sum::<f64>() / n;
        let variance = series.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n;
        let std_dev = variance.sqrt();
        if std_dev == 0.0 {
            return vec![0.0; series.len()];
        }
        series.iter().map(|x| (x - mean) / std_dev).collect()
    }

    /// Full pipeline: align -> difference -> z_normalize.
    /// Returns None if fewer than 60 overlapping points after differencing.
    pub fn prepare_series(
        destruction: &[(NaiveDate, f64)],
        prices: &[(NaiveDate, f64)],
    ) -> Option<(Vec<f64>, Vec<f64>)> {
        let (aligned_dest, aligned_price) = align_series(destruction, prices);
        let diff_dest = difference(&aligned_dest);
        let diff_price = difference(&aligned_price);

        if diff_dest.len() < 60 {
            tracing::debug!(
                destruction_points = destruction.len(),
                price_points = prices.len(),
                aligned_points = aligned_dest.len(),
                differenced_points = diff_dest.len(),
                "prepare_series: insufficient data (need 60)"
            );
            return None;
        }

        tracing::debug!(
            destruction_points = destruction.len(),
            price_points = prices.len(),
            output_points = diff_dest.len(),
            "prepare_series: series prepared"
        );

        let norm_dest = z_normalize(&diff_dest);
        let norm_price = z_normalize(&diff_price);

        Some((norm_dest, norm_price))
    }
}

// ---------------------------------------------------------------------------
// Module: correlation
// ---------------------------------------------------------------------------

pub mod correlation {
    use super::LagCorrelation;

    /// Compute Pearson correlation between two equal-length slices.
    fn pearson(a: &[f64], b: &[f64]) -> f64 {
        let n = a.len() as f64;
        if n == 0.0 {
            return 0.0;
        }
        let mean_a = a.iter().sum::<f64>() / n;
        let mean_b = b.iter().sum::<f64>() / n;

        let mut cov = 0.0;
        let mut var_a = 0.0;
        let mut var_b = 0.0;
        for i in 0..a.len() {
            let da = a[i] - mean_a;
            let db = b[i] - mean_b;
            cov += da * db;
            var_a += da * da;
            var_b += db * db;
        }

        let denom = (var_a * var_b).sqrt();
        if denom == 0.0 {
            0.0
        } else {
            cov / denom
        }
    }

    /// Compute cross-correlation function for lags in [-max_lag, +max_lag].
    ///
    /// For positive lag k: correlate x[0..n-k] with y[k..n]
    ///   (y is shifted forward, meaning x leads y by k steps)
    /// For negative lag k: correlate x[|k|..n] with y[0..n-|k|]
    ///   (x is shifted forward, meaning y leads x by |k| steps)
    pub fn cross_correlation(
        x: &[f64],
        y: &[f64],
        max_lag: i32,
    ) -> Vec<LagCorrelation> {
        let n = x.len().min(y.len());
        tracing::debug!(input_size = n, max_lag, "cross_correlation");
        let mut results = Vec::new();

        for lag in -max_lag..=max_lag {
            let abs_lag = lag.unsigned_abs() as usize;
            if abs_lag >= n {
                results.push(LagCorrelation {
                    lag,
                    correlation: 0.0,
                });
                continue;
            }

            let corr = if lag >= 0 {
                let k = lag as usize;
                pearson(&x[..n - k], &y[k..n])
            } else {
                let k = abs_lag;
                pearson(&x[k..n], &y[..n - k])
            };

            results.push(LagCorrelation {
                lag,
                correlation: corr,
            });
        }

        results
    }

    /// Find the lag with the maximum absolute correlation.
    pub fn find_optimal_lag(ccf: &[LagCorrelation]) -> LagCorrelation {
        ccf.iter()
            .max_by(|a, b| {
                a.correlation
                    .abs()
                    .partial_cmp(&b.correlation.abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .cloned()
            .unwrap_or(LagCorrelation {
                lag: 0,
                correlation: 0.0,
            })
    }

    /// 95% confidence band threshold: 1.96 / sqrt(n).
    pub fn confidence_band(n: usize) -> f64 {
        1.96 / (n as f64).sqrt()
    }
}

// ---------------------------------------------------------------------------
// Module: granger
// ---------------------------------------------------------------------------

pub mod granger {
    use super::GrangerResult;
    use nalgebra::{DMatrix, DVector};
    use statrs::distribution::{ContinuousCDF, FisherSnedecor};

    /// Fit OLS and return residual sum of squares.
    /// Model: y = X * beta + epsilon
    /// beta = (X'X)^(-1) X'y
    pub fn ols_residual_ss(x_matrix: &DMatrix<f64>, y: &DVector<f64>) -> f64 {
        let xt = x_matrix.transpose();
        let xtx = &xt * x_matrix;
        let xty = &xt * y;

        // Solve using Cholesky or fall back to pseudo-inverse
        let beta = match xtx.clone().cholesky() {
            Some(chol) => chol.solve(&xty),
            None => {
                // Fall back to SVD-based pseudo-inverse
                let svd = xtx.svd(true, true);
                svd.solve(&xty, 1e-12).unwrap_or_else(|_| {
                    DVector::zeros(x_matrix.ncols())
                })
            }
        };

        let residuals = y - x_matrix * &beta;
        residuals.dot(&residuals)
    }

    /// Perform Granger causality test.
    ///
    /// Tests whether past values of `x` help predict `y` beyond what past
    /// values of `y` alone can predict.
    ///
    /// Restricted model:  y[t] = c + a1*y[t-1] + ... + ap*y[t-p]
    /// Unrestricted model: y[t] = c + a1*y[t-1] + ... + ap*y[t-p] + b1*x[t-1] + ... + bp*x[t-p]
    pub fn granger_causality(
        y: &[f64],
        x: &[f64],
        max_lag: usize,
    ) -> GrangerResult {
        let n_total = y.len().min(x.len());
        let p = max_lag;

        if n_total <= 2 * p + 1 {
            return GrangerResult {
                f_statistic: 0.0,
                p_value: 1.0,
                significant: false,
                lags_used: p,
            };
        }

        let n = n_total - p; // number of usable observations

        // Build dependent variable vector: y[p], y[p+1], ..., y[n_total-1]
        let y_vec = DVector::from_fn(n, |i, _| y[p + i]);

        // Build restricted design matrix: intercept + y[t-1]..y[t-p]
        let restricted_cols = 1 + p;
        let x_restricted = DMatrix::from_fn(n, restricted_cols, |i, j| {
            if j == 0 {
                1.0 // intercept
            } else {
                y[p + i - j] // y[t - j]
            }
        });

        // Build unrestricted design matrix: intercept + y[t-1]..y[t-p] + x[t-1]..x[t-p]
        let unrestricted_cols = 1 + 2 * p;
        let x_unrestricted = DMatrix::from_fn(n, unrestricted_cols, |i, j| {
            if j == 0 {
                1.0 // intercept
            } else if j <= p {
                y[p + i - j] // y[t - j]
            } else {
                let lag = j - p; // x[t - lag]
                x[p + i - lag]
            }
        });

        let rss_r = ols_residual_ss(&x_restricted, &y_vec);
        let rss_u = ols_residual_ss(&x_unrestricted, &y_vec);

        // Degrees of freedom
        let df1 = p as f64;
        let df2 = (n as f64) - (2.0 * p as f64) - 1.0;

        if df2 <= 0.0 || rss_u <= 0.0 {
            return GrangerResult {
                f_statistic: 0.0,
                p_value: 1.0,
                significant: false,
                lags_used: p,
            };
        }

        let f_stat = ((rss_r - rss_u) / df1) / (rss_u / df2);
        let f_stat = f_stat.max(0.0); // guard against numerical noise

        // p-value from F-distribution
        let p_value = match FisherSnedecor::new(df1, df2) {
            Ok(f_dist) => 1.0 - f_dist.cdf(f_stat),
            Err(_) => 1.0,
        };

        let significant = p_value < 0.05;
        tracing::debug!(f_stat, p_value, significant, lags_used = p, "granger_causality");

        GrangerResult {
            f_statistic: f_stat,
            p_value,
            significant,
            lags_used: p,
        }
    }
}

// ---------------------------------------------------------------------------
// Top-level analysis pipeline
// ---------------------------------------------------------------------------

/// Run the full analysis pipeline on destruction and price time series.
///
/// Returns `None` if the series have fewer than 60 overlapping points after
/// differencing.
pub fn analyze(
    destruction: &[(NaiveDate, f64)],
    prices: &[(NaiveDate, f64)],
) -> Option<AnalysisResult> {
    let (dest, price) = timeseries::prepare_series(destruction, prices)?;

    let max_lag = 30;
    let ccf = correlation::cross_correlation(&dest, &price, max_lag);
    let optimal_lag = correlation::find_optimal_lag(&ccf);

    // Granger lag: absolute value of optimal lag, clamped to [1, 10]
    let granger_lag = (optimal_lag.lag.unsigned_abs() as usize).clamp(1, 10);
    let granger_result = granger::granger_causality(&price, &dest, granger_lag);

    let confidence_threshold = correlation::confidence_band(dest.len());

    debug!(
        optimal_lag = optimal_lag.lag,
        optimal_correlation = optimal_lag.correlation,
        granger_significant = granger_result.significant,
        confidence_threshold,
        "analyze: complete"
    );

    Some(AnalysisResult {
        optimal_lag,
        ccf,
        granger: granger_result,
        confidence_threshold,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn test_difference() {
        let series = vec![1.0, 3.0, 6.0, 10.0];
        let diff = timeseries::difference(&series);
        assert_eq!(diff, vec![2.0, 3.0, 4.0]);
    }

    #[test]
    fn test_z_normalize() {
        let series = vec![2.0, 4.0, 6.0];
        let norm = timeseries::z_normalize(&series);
        assert!((norm[0] + 1.2247).abs() < 0.01);
        assert!(norm[1].abs() < 0.01);
        assert!((norm[2] - 1.2247).abs() < 0.01);
    }

    #[test]
    fn test_z_normalize_constant() {
        let series = vec![5.0, 5.0, 5.0];
        let norm = timeseries::z_normalize(&series);
        assert_eq!(norm, vec![0.0, 0.0, 0.0]);
    }

    #[test]
    fn test_align_series_basic() {
        let dest = vec![
            (date(2024, 1, 1), 100.0),
            (date(2024, 1, 3), 200.0),
        ];
        let prices = vec![
            (date(2024, 1, 1), 10.0),
            (date(2024, 1, 2), 11.0),
        ];
        let (d, p) = timeseries::align_series(&dest, &prices);
        // Overlapping range: Jan 1 to Jan 2 (min of maxes, max of mins)
        // Wait: dest max is Jan 3, price max is Jan 2 -> overlap ends Jan 2
        // dest min is Jan 1, price min is Jan 1 -> overlap starts Jan 1
        // So range is Jan 1, Jan 2
        assert_eq!(d.len(), 2);
        assert_eq!(p.len(), 2);
        assert_eq!(d[0], 100.0); // Jan 1
        assert_eq!(d[1], 0.0);   // Jan 2, missing -> zero fill
        assert_eq!(p[0], 10.0);  // Jan 1
        assert_eq!(p[1], 11.0);  // Jan 2
    }

    #[test]
    fn test_cross_correlation_identity() {
        let x: Vec<f64> = (0..100).map(|i| (i as f64).sin()).collect();
        let ccf = correlation::cross_correlation(&x, &x, 5);
        let optimal = correlation::find_optimal_lag(&ccf);
        assert_eq!(optimal.lag, 0);
        assert!((optimal.correlation - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_confidence_band() {
        let cb = correlation::confidence_band(100);
        assert!((cb - 0.196).abs() < 0.001);
    }

    #[test]
    fn test_prepare_series_too_short() {
        let dest: Vec<(NaiveDate, f64)> = (0..30)
            .map(|i| {
                (
                    date(2024, 1, 1) + chrono::Duration::days(i),
                    i as f64,
                )
            })
            .collect();
        let prices: Vec<(NaiveDate, f64)> = (0..30)
            .map(|i| {
                (
                    date(2024, 1, 1) + chrono::Duration::days(i),
                    100.0 + i as f64,
                )
            })
            .collect();
        assert!(timeseries::prepare_series(&dest, &prices).is_none());
    }

    #[test]
    fn test_granger_no_causality() {
        // Random-ish independent series - Granger test should generally not be significant
        let y: Vec<f64> = (0..200).map(|i| ((i * 7 + 3) % 13) as f64).collect();
        let x: Vec<f64> = (0..200).map(|i| ((i * 11 + 5) % 17) as f64).collect();
        let result = granger::granger_causality(&y, &x, 3);
        assert_eq!(result.lags_used, 3);
        // f_statistic and p_value should be valid numbers
        assert!(result.f_statistic.is_finite());
        assert!(result.p_value.is_finite());
    }

    #[test]
    fn test_full_analyze_pipeline() {
        // Build 100 days of data
        let dest: Vec<(NaiveDate, f64)> = (0..100)
            .map(|i| {
                (
                    date(2024, 1, 1) + chrono::Duration::days(i),
                    (i as f64 * 0.1).sin() * 100.0,
                )
            })
            .collect();
        let prices: Vec<(NaiveDate, f64)> = (0..100)
            .map(|i| {
                (
                    date(2024, 1, 1) + chrono::Duration::days(i),
                    1000.0 + (i as f64 * 0.1).cos() * 50.0,
                )
            })
            .collect();
        let result = analyze(&dest, &prices);
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.ccf.len(), 61); // -30..=+30
        assert!(r.confidence_threshold > 0.0);
        assert!(r.granger.f_statistic.is_finite());
    }

    // -----------------------------------------------------------------------
    // Granger causality edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_granger_insufficient_data() {
        // n_total=5, p=3: 5 <= 2*3+1=7, should bail early
        let y = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let x = vec![5.0, 4.0, 3.0, 2.0, 1.0];
        let result = granger::granger_causality(&y, &x, 3);
        assert_eq!(result.f_statistic, 0.0);
        assert_eq!(result.p_value, 1.0);
        assert!(!result.significant);
        assert_eq!(result.lags_used, 3);
    }

    #[test]
    fn test_granger_identical_series() {
        // y == x: near-singular matrices in OLS
        let series: Vec<f64> = (0..100).map(|i| (i as f64 * 0.3).sin()).collect();
        let result = granger::granger_causality(&series, &series, 3);
        assert!(result.f_statistic.is_finite());
        assert!(result.p_value.is_finite());
    }

    #[test]
    fn test_ols_svd_fallback() {
        use nalgebra::{DMatrix, DVector};
        // Rank-deficient matrix: column 2 == column 1
        let x = DMatrix::from_row_slice(4, 3, &[
            1.0, 2.0, 2.0,
            1.0, 3.0, 3.0,
            1.0, 4.0, 4.0,
            1.0, 5.0, 5.0,
        ]);
        let y = DVector::from_vec(vec![1.0, 2.0, 3.0, 4.0]);
        let rss = granger::ols_residual_ss(&x, &y);
        assert!(rss.is_finite());
        assert!(rss >= 0.0);
    }

    // -----------------------------------------------------------------------
    // analyze() lag clamping
    // -----------------------------------------------------------------------

    #[test]
    fn test_lag_clamping_negative() {
        // (-5i32).unsigned_abs() == 5, clamped to [1,10] == 5
        let lag = (-5i32).unsigned_abs() as usize;
        assert_eq!(lag.clamp(1, 10), 5);
    }

    #[test]
    fn test_lag_clamping_zero() {
        let lag = 0i32.unsigned_abs() as usize;
        assert_eq!(lag.clamp(1, 10), 1);
    }

    #[test]
    fn test_lag_clamping_large() {
        let lag = 25i32.unsigned_abs() as usize;
        assert_eq!(lag.clamp(1, 10), 10);
    }

    // -----------------------------------------------------------------------
    // pearson() edge cases (tested via cross_correlation)
    // -----------------------------------------------------------------------

    #[test]
    fn test_cross_correlation_zero_variance() {
        // One constant series: pearson denominator is 0 → returns 0.0
        let x: Vec<f64> = (0..50).map(|i| i as f64).collect();
        let constant = vec![5.0; 50];
        let ccf = correlation::cross_correlation(&x, &constant, 3);
        for lc in &ccf {
            assert_eq!(lc.correlation, 0.0);
        }
    }

    #[test]
    fn test_cross_correlation_anti_correlated() {
        let x: Vec<f64> = (0..100).map(|i| i as f64).collect();
        let y: Vec<f64> = (0..100).map(|i| -(i as f64)).collect();
        let ccf = correlation::cross_correlation(&x, &y, 0);
        assert_eq!(ccf.len(), 1);
        assert!((ccf[0].correlation - (-1.0)).abs() < 1e-10);
    }

    // -----------------------------------------------------------------------
    // align_series() edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_align_series_disjoint() {
        let dest = vec![
            (date(2024, 1, 1), 10.0),
            (date(2024, 1, 5), 20.0),
        ];
        let prices = vec![
            (date(2024, 6, 1), 100.0),
            (date(2024, 6, 5), 200.0),
        ];
        let (d, p) = timeseries::align_series(&dest, &prices);
        assert!(d.is_empty());
        assert!(p.is_empty());
    }

    #[test]
    fn test_align_series_no_price_seed() {
        // Prices start after destruction starts, no price on or before min_date
        // last_price stays None, so forward-fill uses 0.0
        let dest = vec![
            (date(2024, 1, 1), 10.0),
            (date(2024, 1, 2), 20.0),
            (date(2024, 1, 3), 30.0),
        ];
        let prices = vec![
            (date(2024, 1, 3), 100.0),
        ];
        // Overlap: max(Jan1,Jan3)=Jan3, min(Jan3,Jan3)=Jan3 → single day
        let (d, p) = timeseries::align_series(&dest, &prices);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0], 30.0);
        assert_eq!(p[0], 100.0);
    }

    // -----------------------------------------------------------------------
    // difference() edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_difference_empty() {
        assert!(timeseries::difference(&[]).is_empty());
    }

    #[test]
    fn test_difference_single() {
        assert!(timeseries::difference(&[42.0]).is_empty());
    }

    // -----------------------------------------------------------------------
    // prepare_series() boundary
    // -----------------------------------------------------------------------

    #[test]
    fn test_prepare_series_exactly_60() {
        // 61 aligned points → 60 after differencing → exactly meets threshold
        let dest: Vec<(NaiveDate, f64)> = (0..61)
            .map(|i| (date(2024, 1, 1) + chrono::Duration::days(i), i as f64))
            .collect();
        let prices: Vec<(NaiveDate, f64)> = (0..61)
            .map(|i| (date(2024, 1, 1) + chrono::Duration::days(i), 100.0 + i as f64))
            .collect();
        assert!(timeseries::prepare_series(&dest, &prices).is_some());
    }

    // -----------------------------------------------------------------------
    // cross_correlation with known shifted signal
    // -----------------------------------------------------------------------

    #[test]
    fn test_cross_correlation_known_shift() {
        // y = x delayed by 3 steps
        let x: Vec<f64> = (0..100).map(|i| (i as f64 * 0.2).sin()).collect();
        let mut y = vec![0.0; 3];
        y.extend_from_slice(&x[..97]);
        let ccf = correlation::cross_correlation(&x, &y, 10);
        let optimal = correlation::find_optimal_lag(&ccf);
        // Positive lag means x leads y, which matches our construction
        assert_eq!(optimal.lag, 3);
    }
}
