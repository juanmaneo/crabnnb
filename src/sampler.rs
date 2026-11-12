use std::time::Duration;

use crate::distance;

pub struct Sampler {
    pub num_samples: usize,
    pub sample_sleep: Duration,
    pub max_attempts: usize,
    pub measured_power: f64,
    prev_avg_rssi: f64,
    prev_avg_distance: f64,
    prev_tx_power: f64,
}

impl Sampler {
    pub fn new(measured_power: f64) -> Self {
        Sampler::with_params(measured_power, 7, Duration::from_millis(5), 70)
    }

    pub fn with_params(
        measured_power: f64,
        num_samples: usize,
        sample_sleep: Duration,
        max_attempts: usize,
    ) -> Self {
        Sampler {
            num_samples,
            sample_sleep,
            max_attempts,
            measured_power,
            prev_avg_rssi: measured_power,
            prev_avg_distance: 1.0,
            prev_tx_power: measured_power,
        }
    }

    pub fn record_sample(&mut self, rssi: f64, tx: Option<f64>) -> f64 {
        let d1 = distance::rssi_to_distance(rssi, self.measured_power);
        let tx_power = tx.unwrap_or(self.prev_tx_power);
        self.prev_tx_power = tx_power;
        let d2 = distance::calculate_accuracy(tx_power, rssi, self.measured_power);
        if d2 > 1.0 {
            d1
        } else {
            (d1 * 3.0 + d2) / 4.0
        }
    }

    pub fn finish(&mut self, samples: &[(f64, f64)]) -> f64 {
        if samples.is_empty() {
            self.prev_avg_distance
        } else {
            let n = samples.len() as f64;
            let avg_rssi = samples.iter().map(|s| s.0).sum::<f64>() / n;
            let avg_distance = samples.iter().map(|s| s.1).sum::<f64>() / n;
            self.prev_avg_rssi = avg_rssi;
            self.prev_avg_distance = avg_distance;
            avg_distance
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_defaults() {
        let s = Sampler::new(-59.0);
        assert_eq!(s.num_samples, 7);
        assert_eq!(s.sample_sleep, Duration::from_millis(5));
        assert_eq!(s.max_attempts, 70);
        assert!((s.measured_power - (-59.0)).abs() < 1e-10);
        assert!((s.prev_avg_rssi - (-59.0)).abs() < 1e-10);
        assert!((s.prev_avg_distance - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_with_params() {
        let s = Sampler::with_params(-65.0, 5, Duration::from_millis(100), 50);
        assert_eq!(s.num_samples, 5);
        assert_eq!(s.sample_sleep, Duration::from_millis(100));
        assert_eq!(s.max_attempts, 50);
        assert!((s.measured_power - (-65.0)).abs() < 1e-10);
    }

    #[test]
    fn test_record_sample_with_tx() {
        let mut s = Sampler::new(-59.0);
        let d = s.record_sample(-65.0, Some(-59.0));
        assert!(d > 0.0);
    }

    #[test]
    fn test_record_sample_without_tx() {
        let mut s = Sampler::new(-59.0);
        let d = s.record_sample(-65.0, None);
        assert!(d > 0.0);
    }

    #[test]
    fn test_finish_empty_uses_previous() {
        let mut s = Sampler::new(-59.0);
        s.prev_avg_distance = 2.5;
        let d = s.finish(&[]);
        assert!((d - 2.5).abs() < 1e-10);
    }

    #[test]
    fn test_finish_averages_samples() {
        let mut s = Sampler::new(-59.0);
        let samples = vec![(-60.0, 1.0), (-62.0, 2.0), (-64.0, 3.0)];
        let d = s.finish(&samples);
        assert!((d - 2.0).abs() < 1e-10); // avg of 1,2,3
        assert!((s.prev_avg_rssi - (-62.0)).abs() < 1e-10); // avg of -60,-62,-64
        assert!((s.prev_avg_distance - 2.0).abs() < 1e-10);
    }

    #[test]
    fn test_record_then_finish() {
        let mut s = Sampler::new(-59.0);
        let d1 = s.record_sample(-60.0, Some(-59.0));
        let d2 = s.record_sample(-65.0, None);
        let samples = vec![(-60.0, d1), (-65.0, d2)];
        let avg = s.finish(&samples);
        assert!((avg - (d1 + d2) / 2.0).abs() < 1e-10);
    }
}
