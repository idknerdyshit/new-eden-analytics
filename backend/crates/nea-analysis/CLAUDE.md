# nea-analysis

Statistical analysis engine — cross-correlation and Granger causality testing for destruction→price relationships.

## Responsibilities

- Time series alignment and preprocessing (forward-fill, differencing, normalization)
- Cross-correlation function (CCF) computation
- Granger causality testing via OLS regression + F-test
- Orchestrating analysis runs across all item pairs

## Modules

| Module | Contents |
|--------|----------|
| `timeseries` | `align_series`, `difference`, `z_normalize`, `prepare_series` |
| `correlation` | `pearson`, `cross_correlation`, `find_optimal_lag`, `confidence_band` |
| `granger` | `ols_residual_ss`, `granger_causality` |
| `runner` | `run_analysis(pool, region_id)`, `analyze_pair` — DB-aware orchestration |

## Key Types

- `LagCorrelation` — lag (i32), correlation (f64)
- `GrangerResult` — f_statistic, p_value, significant (bool at p<0.05), lags_used
- `AnalysisResult` — optimal_lag, ccf (Vec<LagCorrelation>), granger, confidence_threshold

## Analysis Pipeline (`analyze()`)

1. `prepare_series` — align dates, first-order difference, z-normalize (requires ≥60 points)
2. `cross_correlation` — CCF for lags in [-30, +30]
3. `find_optimal_lag` — max |correlation| across all lags
4. `granger_causality` — OLS F-test with lag clamped to [1, 10]
5. `confidence_band` — 95% threshold (1.96 / √n)

## Granger Causality

- Restricted model: y[t] = c + Σ a_i·y[t-i]
- Unrestricted model: y[t] = c + Σ a_i·y[t-i] + Σ b_i·x[t-i]
- F-test: ((RSS_r - RSS_u) / p) / (RSS_u / df2)
- OLS solved via Cholesky decomposition with SVD fallback for numerical stability

## CCF Sign Convention

- Positive lag k: x (destruction) leads y (price) by k days
- Negative lag k: y (price) leads x (destruction) by k days

## Tests

12 unit tests covering differencing, normalization, alignment, cross-correlation, and confidence bands.

## Dependencies

External: nalgebra, statrs, chrono, serde, thiserror, tracing
Workspace: nea-db (used by `runner` module for DB queries)
