//! Numbers from metric points: percentile, mean, throughput from a byte counter.

/// Nearest-rank percentile (`p` in 0–1); `None` for no points.
pub fn percentile(values: &[f64], p: f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    let rank = ((p * v.len() as f64).ceil() as usize).clamp(1, v.len());
    Some(v[rank - 1])
}

/// Arithmetic mean; `None` for no points.
pub fn mean(values: &[f64]) -> Option<f64> {
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}

/// Bits per second between consecutive readings of a cumulative byte counter taken
/// `interval_secs` apart. A drop means the counter reset (reboot): that step is skipped.
pub fn counter_rates_bps(readings: &[f64], interval_secs: u64) -> Vec<f64> {
    readings
        .windows(2)
        .filter_map(|w| {
            let delta = w[1] - w[0];
            (delta >= 0.0).then(|| delta * 8.0 / interval_secs as f64)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles() {
        let v: Vec<f64> = (1..=100).map(f64::from).collect();
        assert_eq!(percentile(&v, 0.95), Some(95.0));
        assert_eq!(percentile(&[3.0], 0.95), Some(3.0));
        assert_eq!(percentile(&[], 0.95), None);
        assert_eq!(mean(&[1.0, 2.0, 6.0]), Some(3.0));
    }

    #[test]
    fn counter_resets_are_skipped() {
        // 300 s apart: +3000 bytes = 80 bit/s; then a reboot resets the counter.
        let r = counter_rates_bps(&[1000.0, 4000.0, 7000.0, 50.0, 3050.0], 300);
        assert_eq!(r, vec![80.0, 80.0, 80.0]);
    }
}
