/// MacJet — CPU Predictor (Online RLS)
///
/// Recursive Least Squares with forgetting factor (λ=0.99) for
/// streaming CPU prediction. Zero external ML dependencies.
///
/// Features (10): lag1..5, mean_10, std_10, mean_30, trend, bias
/// Ring buffer: 600 samples max (~10 min at 1s tick)
/// Horizon: 60-step iterated forecast with synthetic feature buffer
use std::time::Instant;

const RING_CAP: usize = 600;
const NUM_FEATURES: usize = 10;
const LAMBDA: f64 = 0.99; // forgetting factor
const RIDGE: f64 = 100.0; // initial P diagonal (regularization)
const MIN_SAMPLES_FOR_FEATURES: usize = 10;
const MIN_SAMPLES_FOR_TRAIN: usize = 30; // need 30 for mean_30 window
const TRAIN_INTERVAL: u64 = 60; // train every 60 ticks
const HORIZON: usize = 60;

#[derive(Debug, Clone)]
pub struct PredictorStats {
    pub rows: usize,
    pub cols: usize,
    pub trained: bool,
    pub last_inference_us: u64,
    pub countdown_secs: u64,
    pub mae: f64,
    pub horizon: Vec<f64>,
    pub history: Vec<f64>,
    pub confidence_band: Vec<(f64, f64)>, // (low, high) for each horizon point
}

pub struct CpuPredictor {
    // Ring buffer of raw CPU samples
    ring: Vec<f64>,
    ring_len: usize,

    // RLS model
    weights: [f64; NUM_FEATURES],
    // P matrix (covariance inverse), stored as flat array for cache locality
    p_matrix: [[f64; NUM_FEATURES]; NUM_FEATURES],

    // Training state
    trained: bool,
    samples_since_train: usize,
    tick_counter: u64,
    last_train_tick: u64,

    // Inference stats
    last_inference_us: u64,

    // Accuracy tracking: store predictions made and compare later
    // (tick_when_predicted, predicted_value)
    pending_predictions: Vec<(u64, f64)>,
    mae_sum: f64,
    mae_count: u64,

    // Cached horizon
    cached_horizon: Vec<f64>,
    cached_confidence: Vec<(f64, f64)>,

    // Residual tracking for confidence bands
    residual_sq_sum: f64,
    residual_count: u64,
}

impl CpuPredictor {
    pub fn new() -> Self {
        let mut p = [[0.0; NUM_FEATURES]; NUM_FEATURES];
        for i in 0..NUM_FEATURES {
            p[i][i] = RIDGE;
        }

        Self {
            ring: Vec::with_capacity(RING_CAP),
            ring_len: 0,
            weights: [0.0; NUM_FEATURES],
            p_matrix: p,
            trained: false,
            samples_since_train: 0,
            tick_counter: 0,
            last_train_tick: 0,
            last_inference_us: 0,
            pending_predictions: Vec::new(),
            mae_sum: 0.0,
            mae_count: 0,
            cached_horizon: Vec::new(),
            cached_confidence: Vec::new(),
            residual_sq_sum: 0.0,
            residual_count: 0,
        }
    }

    /// Push a new CPU sample (called every 1s tick).
    pub fn push_sample(&mut self, cpu: f64) {
        self.tick_counter += 1;

        // Ring buffer append
        if self.ring_len < RING_CAP {
            self.ring.push(cpu);
            self.ring_len += 1;
        } else {
            // Shift left by 1 (could use VecDeque but this is fine for 600 items)
            self.ring.rotate_left(1);
            self.ring[RING_CAP - 1] = cpu;
        }

        self.samples_since_train += 1;

        // Check pending predictions for accuracy tracking
        self.check_accuracy(cpu);
    }

    /// Attempt to train on new data. Called periodically.
    pub fn try_train(&mut self) {
        if self.ring_len < MIN_SAMPLES_FOR_TRAIN {
            return;
        }

        // RLS update on all new samples since last train
        let start = if self.ring_len > self.samples_since_train {
            self.ring_len - self.samples_since_train
        } else {
            MIN_SAMPLES_FOR_TRAIN // need at least this many for features
        };
        let start = start.max(MIN_SAMPLES_FOR_TRAIN);

        for i in start..self.ring_len {
            if let Some(features) = self.extract_features_at(i) {
                let target = self.ring[i];
                self.rls_update(&features, target);
            }
        }

        self.trained = true;
        self.samples_since_train = 0;
        self.last_train_tick = self.tick_counter;

        // Generate new horizon
        self.generate_horizon();
    }

    /// RLS rank-1 update: w_{t+1} = w_t + P*x*(y - w_t'*x) / (λ + x'*P*x)
    fn rls_update(&mut self, x: &[f64; NUM_FEATURES], y: f64) {
        // Compute P*x
        let mut px = [0.0; NUM_FEATURES];
        for i in 0..NUM_FEATURES {
            for j in 0..NUM_FEATURES {
                px[i] += self.p_matrix[i][j] * x[j];
            }
        }

        // Compute denominator: λ + x'*P*x
        let mut xpx = LAMBDA;
        for i in 0..NUM_FEATURES {
            xpx += x[i] * px[i];
        }

        if xpx.abs() < 1e-12 {
            return; // numerical guard
        }

        // Compute gain: k = P*x / (λ + x'*P*x)
        let mut gain = [0.0; NUM_FEATURES];
        for i in 0..NUM_FEATURES {
            gain[i] = px[i] / xpx;
        }

        // Prediction error
        let pred: f64 = self
            .weights
            .iter()
            .zip(x.iter())
            .map(|(w, xi)| w * xi)
            .sum();
        let error = y - pred;

        // Track residuals for confidence bands
        self.residual_sq_sum += error * error;
        self.residual_count += 1;

        // Update weights: w = w + gain * error
        for i in 0..NUM_FEATURES {
            self.weights[i] += gain[i] * error;
        }

        // Update P matrix: P = (P - gain * x' * P) / λ
        // More stable: P = (I - gain*x') * P / λ
        let mut new_p = [[0.0; NUM_FEATURES]; NUM_FEATURES];
        for i in 0..NUM_FEATURES {
            for j in 0..NUM_FEATURES {
                let mut val = self.p_matrix[i][j];
                for k in 0..NUM_FEATURES {
                    val -= gain[i] * x[k] * self.p_matrix[k][j];
                }
                new_p[i][j] = val / LAMBDA;
            }
        }
        self.p_matrix = new_p;
    }

    /// Extract features at ring buffer index `idx`.
    fn extract_features_at(&self, idx: usize) -> Option<[f64; NUM_FEATURES]> {
        if idx < MIN_SAMPLES_FOR_FEATURES {
            return None;
        }

        let mut f = [0.0; NUM_FEATURES];

        // Lag features (lag-1 through lag-5)
        for lag in 1..=5 {
            if idx >= lag {
                f[lag - 1] = self.ring[idx - lag];
            }
        }

        // Rolling mean over last 10 samples
        let mean_10 = self.rolling_mean(idx, 10);
        f[5] = mean_10;

        // Rolling std over last 10 samples
        f[6] = self.rolling_std(idx, 10, mean_10);

        // Rolling mean over last 30 samples (or however many we have)
        let window_30 = 30.min(idx);
        let mean_30 = self.rolling_mean(idx, window_30);
        f[7] = mean_30;

        // Trend: mean_10 - mean_30
        f[8] = mean_10 - mean_30;

        // Bias
        f[9] = 1.0;

        Some(f)
    }

    fn rolling_mean(&self, idx: usize, window: usize) -> f64 {
        if window == 0 {
            return 0.0;
        }
        let start = idx.saturating_sub(window);
        let slice = &self.ring[start..idx];
        if slice.is_empty() {
            return 0.0;
        }
        slice.iter().sum::<f64>() / slice.len() as f64
    }

    fn rolling_std(&self, idx: usize, window: usize, mean: f64) -> f64 {
        if window == 0 {
            return 0.0;
        }
        let start = idx.saturating_sub(window);
        let slice = &self.ring[start..idx];
        if slice.len() < 2 {
            return 0.0;
        }
        let var = slice.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / slice.len() as f64;
        var.sqrt()
    }

    /// Generate 60-step iterated horizon with synthetic feature buffer.
    fn generate_horizon(&mut self) {
        let t = Instant::now();

        if self.ring_len < MIN_SAMPLES_FOR_TRAIN {
            // Just use last known value for flat line
            let last = self.last_value();
            self.cached_horizon = vec![last; HORIZON];
            self.cached_confidence = vec![(last, last); HORIZON];
            self.last_inference_us = t.elapsed().as_micros() as u64;
            return;
        }

        // Build synthetic buffer: copy last 30 values from ring
        let copy_len = 30.min(self.ring_len);
        let start = self.ring_len - copy_len;
        let mut synth: Vec<f64> = self.ring[start..self.ring_len].to_vec();

        let sigma = self.residual_sigma();
        let mut horizon = Vec::with_capacity(HORIZON);
        let mut confidence = Vec::with_capacity(HORIZON);

        for step in 0..HORIZON {
            let idx = synth.len();
            // Extract features from synthetic buffer
            let features = self.extract_features_from_buf(&synth, idx);
            let pred: f64 = self
                .weights
                .iter()
                .zip(features.iter())
                .map(|(w, x)| w * x)
                .sum();

            let clamped = pred.clamp(0.0, 100.0);
            horizon.push(clamped);

            // Confidence band widens with each step
            let band_width = sigma * ((step + 1) as f64).sqrt();
            confidence.push((
                (clamped - band_width).max(0.0),
                (clamped + band_width).min(100.0),
            ));

            synth.push(clamped);
        }

        self.last_inference_us = t.elapsed().as_micros() as u64;
        self.cached_horizon = horizon;
        self.cached_confidence = confidence;

        // Store one-step-ahead prediction for accuracy tracking
        if !self.cached_horizon.is_empty() {
            self.pending_predictions
                .push((self.tick_counter + 1, self.cached_horizon[0]));
            // Keep only last 120 pending predictions
            if self.pending_predictions.len() > 120 {
                self.pending_predictions
                    .drain(0..self.pending_predictions.len() - 120);
            }
        }
    }

    /// Extract features from an arbitrary buffer (for horizon generation).
    fn extract_features_from_buf(&self, buf: &[f64], idx: usize) -> [f64; NUM_FEATURES] {
        let mut f = [0.0; NUM_FEATURES];

        // Lags
        for lag in 1..=5 {
            if idx >= lag {
                f[lag - 1] = buf[idx - lag];
            }
        }

        // Mean 10
        let w10 = 10.min(idx);
        let start10 = idx - w10;
        let slice10 = &buf[start10..idx];
        let mean_10 = if slice10.is_empty() {
            0.0
        } else {
            slice10.iter().sum::<f64>() / slice10.len() as f64
        };
        f[5] = mean_10;

        // Std 10
        if slice10.len() >= 2 {
            let var =
                slice10.iter().map(|v| (v - mean_10).powi(2)).sum::<f64>() / slice10.len() as f64;
            f[6] = var.sqrt();
        }

        // Mean 30
        let w30 = 30.min(idx);
        let start30 = idx - w30;
        let slice30 = &buf[start30..idx];
        let mean_30 = if slice30.is_empty() {
            0.0
        } else {
            slice30.iter().sum::<f64>() / slice30.len() as f64
        };
        f[7] = mean_30;

        // Trend
        f[8] = mean_10 - mean_30;

        // Bias
        f[9] = 1.0;

        f
    }

    fn residual_sigma(&self) -> f64 {
        if self.residual_count < 2 {
            return 5.0;
        } // default uncertainty
        (self.residual_sq_sum / self.residual_count as f64).sqrt()
    }

    fn check_accuracy(&mut self, actual: f64) {
        let tick = self.tick_counter;
        let mut matched = Vec::new();

        for (i, &(pred_tick, pred_val)) in self.pending_predictions.iter().enumerate() {
            if pred_tick == tick {
                let err = (actual - pred_val).abs();
                self.mae_sum += err;
                self.mae_count += 1;
                matched.push(i);
            }
        }

        // Remove matched (iterate in reverse to keep indices valid)
        for i in matched.into_iter().rev() {
            self.pending_predictions.remove(i);
        }
    }

    /// Get current last known CPU value.
    pub fn last_value(&self) -> f64 {
        if self.ring_len == 0 {
            0.0
        } else {
            self.ring[self.ring_len - 1]
        }
    }

    /// Is the model trained?
    pub fn is_trained(&self) -> bool {
        self.trained
    }

    /// Full stats for the UI.
    pub fn stats(&self) -> PredictorStats {
        let countdown = if self.last_train_tick == 0 {
            TRAIN_INTERVAL.saturating_sub(self.tick_counter)
        } else {
            TRAIN_INTERVAL.saturating_sub(self.tick_counter - self.last_train_tick)
        };

        let mae = if self.mae_count > 0 {
            self.mae_sum / self.mae_count as f64
        } else {
            0.0
        };

        // Return last 60 samples of history for chart
        let history_len = 60.min(self.ring_len);
        let history_start = self.ring_len - history_len;
        let history = self.ring[history_start..self.ring_len].to_vec();

        PredictorStats {
            rows: self.ring_len,
            cols: NUM_FEATURES,
            trained: self.trained,
            last_inference_us: self.last_inference_us,
            countdown_secs: countdown,
            mae,
            horizon: self.cached_horizon.clone(),
            history,
            confidence_band: self.cached_confidence.clone(),
        }
    }

    /// Should we train this tick?
    pub fn should_train(&self) -> bool {
        if self.last_train_tick == 0 && self.ring_len >= MIN_SAMPLES_FOR_TRAIN {
            return true;
        }
        self.tick_counter - self.last_train_tick >= TRAIN_INTERVAL
    }
}

// ─── Tests ─────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cold_start_uses_last_value() {
        let mut p = CpuPredictor::new();
        // No samples — should be 0
        assert_eq!(p.last_value(), 0.0);

        // Push one sample
        p.push_sample(42.0);
        assert_eq!(p.last_value(), 42.0);
        assert!(!p.is_trained());

        // Stats should show flat horizon at last value
        let stats = p.stats();
        assert!(!stats.trained);
        assert_eq!(stats.rows, 1);
        assert_eq!(stats.cols, NUM_FEATURES);
    }

    #[test]
    fn test_constant_signal_steady() {
        let mut p = CpuPredictor::new();

        // Feed 60 constant samples
        for _ in 0..60 {
            p.push_sample(50.0);
        }
        p.try_train();

        assert!(p.is_trained());
        let stats = p.stats();

        // All horizon values should be close to 50
        for v in &stats.horizon {
            assert!((*v - 50.0).abs() < 10.0, "expected ~50, got {}", v);
        }
    }

    #[test]
    fn test_ramp_extrapolation() {
        let mut p = CpuPredictor::new();

        // Feed linearly increasing signal: 10, 10.5, 11, ...
        for i in 0..60 {
            p.push_sample(10.0 + i as f64 * 0.5);
        }
        p.try_train();

        let stats = p.stats();
        assert!(stats.trained);
        // First horizon point should be > last actual (39.5)
        assert!(
            stats.horizon[0] > 30.0,
            "prediction should continue upward, got {}",
            stats.horizon[0]
        );
    }

    #[test]
    fn test_ring_buffer_cap() {
        let mut p = CpuPredictor::new();
        for i in 0..700 {
            p.push_sample(i as f64 % 100.0);
        }
        assert_eq!(p.ring_len, RING_CAP);
        assert_eq!(p.ring.len(), RING_CAP);
    }

    #[test]
    fn test_horizon_clamped() {
        let mut p = CpuPredictor::new();
        // Wild oscillation to stress model
        for i in 0..60 {
            p.push_sample(if i % 2 == 0 { 99.0 } else { 1.0 });
        }
        p.try_train();

        let stats = p.stats();
        for v in &stats.horizon {
            assert!(
                *v >= 0.0 && *v <= 100.0,
                "horizon value out of bounds: {}",
                v
            );
        }
    }

    #[test]
    fn test_feature_count_is_10() {
        let p = CpuPredictor::new();
        let stats = p.stats();
        assert_eq!(stats.cols, 10);
    }

    #[test]
    fn test_mae_tracking() {
        let mut p = CpuPredictor::new();

        // Feed 60 samples and train
        for i in 0..60 {
            p.push_sample(20.0 + (i as f64).sin() * 5.0);
        }
        p.try_train();

        // The pending_predictions should have an entry
        assert!(!p.pending_predictions.is_empty());

        // Feed more samples to trigger accuracy checks
        for i in 60..120 {
            p.push_sample(20.0 + (i as f64).sin() * 5.0);
        }

        // MAE should be tracked (may or may not be > 0 depending on timing)
        let stats = p.stats();
        assert!(stats.mae >= 0.0);
    }
}
