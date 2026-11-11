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
