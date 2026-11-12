pub const PATH_LOSS_EXPONENT: f64 = 2.5;

pub fn rssi_to_distance(rssi: f64, measured_power: f64) -> f64 {
    let x = (measured_power - rssi) / (10.0 * PATH_LOSS_EXPONENT);
    10f64.powf(x)
}

fn accuracy_curve(tx_power: f64, rssi: f64, measured_power: f64, a: f64, b: f64, c: f64) -> f64 {
    if tx_power == 0.0 {
        return rssi_to_distance(rssi, measured_power);
    }
    let ratio = rssi / tx_power;
    if ratio < 0.0 {
        return rssi_to_distance(rssi, measured_power);
    }
    a * ratio.powf(b) + c
}

pub fn old_accuracy(tx_power: f64, rssi: f64, measured_power: f64) -> f64 {
    accuracy_curve(tx_power, rssi, measured_power, 0.89976, 7.7095, 0.111)
}

pub fn new_accuracy(tx_power: f64, rssi: f64, measured_power: f64) -> f64 {
    accuracy_curve(tx_power, rssi, measured_power, 0.42093, 6.9476, 0.54992)
}

pub fn calculate_accuracy(tx_power: f64, rssi: f64, measured_power: f64) -> f64 {
    (old_accuracy(tx_power, rssi, measured_power) + new_accuracy(tx_power, rssi, measured_power))
        / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rssi_to_distance_at_1m() {
        // At measured power, distance should be exactly 1 meter
        let d = rssi_to_distance(-59.0, -59.0);
        assert!((d - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_rssi_to_distance_weaker_signal() {
        // Weaker signal (more negative) should give larger distance
        let d = rssi_to_distance(-69.0, -59.0);
        assert!(d > 1.0);
        // 10 dB weaker with n=2.5 → 10^(10/25) ≈ 2.51 m
        assert!((d - 10f64.powf(10.0 / 25.0)).abs() < 1e-10);
    }

    #[test]
    fn test_rssi_to_distance_stronger_signal() {
        // Stronger signal should give smaller distance
        let d = rssi_to_distance(-49.0, -59.0);
        assert!(d < 1.0);
    }

    #[test]
    fn test_accuracy_curve_zero_tx_power() {
        // Falls back to rssi_to_distance when tx_power is 0
        let d = accuracy_curve(0.0, -59.0, -59.0, 1.0, 1.0, 1.0);
        assert!((d - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_accuracy_curve_negative_ratio() {
        // Falls back when ratio is negative (rssi < 0, tx_power > 0 is impossible
        // since both are negative dBm; but rssi/tx_power < 0 when signs differ)
        let d = accuracy_curve(-10.0, 5.0, -59.0, 1.0, 1.0, 1.0);
        let fallback = rssi_to_distance(5.0, -59.0);
        assert!((d - fallback).abs() < 1e-10);
    }

    #[test]
    fn test_old_and_new_accuracy_differ() {
        let old = old_accuracy(-59.0, -65.0, -59.0);
        let new = new_accuracy(-59.0, -65.0, -59.0);
        assert!((old - new).abs() > 1e-6);
    }

    #[test]
    fn test_calculate_accuracy_average() {
        let tx = -59.0;
        let rssi = -65.0;
        let mp = -59.0;
        let expected = (old_accuracy(tx, rssi, mp) + new_accuracy(tx, rssi, mp)) / 2.0;
        let actual = calculate_accuracy(tx, rssi, mp);
        assert!((actual - expected).abs() < 1e-10);
    }
}
