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
