/// Rolling Hurst exponent via Rescaled Range (R/S) analysis on strategy-bar closes.
///
/// H > 0.5 → trending (persistent), H ≈ 0.5 → random walk, H < 0.5 → mean-reverting.
/// Used to detect trend exhaustion in open positions.
///
/// Supports multi-scale confirmation: compute H at different window sizes
/// from the same buffer, require all to agree before confirming regime change.
#[derive(Clone)]
pub struct HurstTracker {
    buf: Vec<f64>,
    len: usize,
    head: usize,
    count: usize,
}

impl HurstTracker {
    pub fn new(window: usize) -> Self {
        Self {
            buf: vec![0.0; window],
            len: window,
            head: 0,
            count: 0,
        }
    }

    pub fn push(&mut self, close: f64) {
        self.buf[self.head] = close;
        self.head = (self.head + 1) % self.len;
        if self.count < self.len {
            self.count += 1;
        }
    }

    /// Compute Hurst at a single window (uses the most recent `n` values
    /// from the ring buffer, or all available if n > count).
    pub fn hurst(&self) -> Option<f64> {
        self.hurst_at(self.count)
    }

    /// Multi-scale Hurst: compute H at each window size, return the maximum.
    /// If the max is below threshold, ALL scales agree on mean-reversion.
    /// Returns None if any scale cannot be computed.
    pub fn hurst_max(&self, windows: &[usize]) -> Option<f64> {
        let mut max_h = f64::NEG_INFINITY;
        for &w in windows {
            let h = self.hurst_at(w)?;
            if h > max_h {
                max_h = h;
            }
        }
        if max_h == f64::NEG_INFINITY {
            None
        } else {
            Some(max_h)
        }
    }

    fn hurst_at(&self, window: usize) -> Option<f64> {
        let n = window.min(self.count);
        if n < 32 {
            return None;
        }

        let returns = self.log_returns_last(n);
        if returns.len() < 16 {
            return None;
        }

        let mut log_n = Vec::new();
        let mut log_rs = Vec::new();

        let mut scale = 8usize;
        while scale <= returns.len() / 2 {
            let avg_rs = rs_at_scale(&returns, scale);
            if avg_rs > 0.0 {
                log_n.push((scale as f64).ln());
                log_rs.push(avg_rs.ln());
            }
            scale *= 2;
        }

        if log_n.len() < 2 {
            return None;
        }

        let h = slope(&log_n, &log_rs);
        Some(h.clamp(0.0, 1.0))
    }

    /// Log returns from the most recent `n` values in the ring buffer.
    fn log_returns_last(&self, n: usize) -> Vec<f64> {
        let n = n.min(self.count);
        let mut returns = Vec::with_capacity(n - 1);
        let start = if self.count == self.len {
            (self.head + self.len - n) % self.len
        } else {
            self.count - n
        };

        let mut prev = self.buf[start % self.len];
        for i in 1..n {
            let idx = (start + i) % self.len;
            let cur = self.buf[idx];
            if prev > 0.0 && cur > 0.0 {
                returns.push((cur / prev).ln());
            }
            prev = cur;
        }
        returns
    }
}

/// Average R/S ratio for a given sub-period scale.
fn rs_at_scale(returns: &[f64], scale: usize) -> f64 {
    let segments = returns.len() / scale;
    if segments == 0 {
        return 0.0;
    }

    let mut rs_sum = 0.0;
    let mut valid = 0usize;

    for seg in 0..segments {
        let start = seg * scale;
        let chunk = &returns[start..start + scale];

        let mean: f64 = chunk.iter().sum::<f64>() / scale as f64;

        let mut cum = 0.0;
        let mut max_cum = f64::NEG_INFINITY;
        let mut min_cum = f64::INFINITY;
        let mut var_sum = 0.0;

        for &r in chunk {
            let dev = r - mean;
            cum += dev;
            if cum > max_cum { max_cum = cum; }
            if cum < min_cum { min_cum = cum; }
            var_sum += dev * dev;
        }

        let range = max_cum - min_cum;
        let std_dev = (var_sum / scale as f64).sqrt();

        if std_dev > 1e-15 {
            rs_sum += range / std_dev;
            valid += 1;
        }
    }

    if valid == 0 { 0.0 } else { rs_sum / valid as f64 }
}

/// OLS slope of y on x.
fn slope(x: &[f64], y: &[f64]) -> f64 {
    let n = x.len() as f64;
    let sx: f64 = x.iter().sum();
    let sy: f64 = y.iter().sum();
    let sxy: f64 = x.iter().zip(y.iter()).map(|(a, b)| a * b).sum();
    let sxx: f64 = x.iter().map(|a| a * a).sum();
    let denom = n * sxx - sx * sx;
    if denom.abs() < 1e-15 { 0.5 } else { (n * sxy - sx * sy) / denom }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_returns_none() {
        let t = HurstTracker::new(64);
        assert!(t.hurst().is_none());
    }

    #[test]
    fn trending_series_high_hurst() {
        let mut t = HurstTracker::new(128);
        let mut price = 100.0;
        for _ in 0..128 {
            price += 0.3;
            t.push(price);
        }
        let h = t.hurst().unwrap();
        assert!(h > 0.5, "trending series should have H > 0.5, got {h}");
    }

    #[test]
    fn mean_reverting_series_low_hurst() {
        let mut t = HurstTracker::new(128);
        for i in 0..128 {
            let price = 100.0 + if i % 2 == 0 { 1.0 } else { -1.0 };
            t.push(price);
        }
        let h = t.hurst().unwrap();
        assert!(h < 0.5, "mean-reverting series should have H < 0.5, got {h}");
    }

    #[test]
    fn ring_buffer_overwrites() {
        let mut t = HurstTracker::new(64);
        let mut price = 100.0;
        for _ in 0..200 {
            price += 0.2;
            t.push(price);
        }
        assert_eq!(t.count, 64);
        assert!(t.hurst().is_some());
    }

    #[test]
    fn hurst_max_multi_scale() {
        let mut t = HurstTracker::new(256);
        let mut price = 100.0;
        for _ in 0..256 {
            price += 0.5;
            t.push(price);
        }
        let h = t.hurst_max(&[48, 128]).unwrap();
        assert!(h > 0.5, "trending series max(H) should be > 0.5, got {h}");
    }

    #[test]
    fn hurst_max_empty_windows_returns_none() {
        let t = HurstTracker::new(64);
        assert!(t.hurst_max(&[48, 64]).is_none());
    }

    #[test]
    fn hurst_max_single_window_equals_hurst_at() {
        let mut t = HurstTracker::new(128);
        let mut price = 100.0;
        for _ in 0..128 {
            price += 0.3;
            t.push(price);
        }
        let h_full = t.hurst().unwrap();
        let h_max = t.hurst_max(&[128]).unwrap();
        assert!((h_full - h_max).abs() < 1e-10);
    }

    #[test]
    fn not_enough_data_returns_none() {
        let mut t = HurstTracker::new(64);
        for i in 0..20 {
            t.push(100.0 + i as f64);
        }
        assert!(t.hurst().is_none(), "< 32 data points should return None");
    }
}
