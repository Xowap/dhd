/// Linear regression of a slice, which helps in finding whether there is a
/// trend in the dataset. Useful for calibration.
pub fn circular_linear_regression(data: &[u16], head: usize) -> Option<(f64, f64)> {
    let n = data.len() as f64;
    if n < 2.0 { return None; }

    let mut sum_x = 0.0;
    let mut sum_y = 0.0;
    let mut sum_xy = 0.0;
    let mut sum_xx = 0.0;

    // We iterate from 0 to N-1 (our logical X-axis)
    // but we map that to the physical index in the circular buffer
    for x_logical in 0..data.len() {
        // The oldest data is at 'head', the newest is at 'head - 1'
        let physical_idx = (head + x_logical) % data.len();

        let x = x_logical as f64;
        let y = data[physical_idx] as f64;

        sum_x += x;
        sum_y += y;
        sum_xy += x * y;
        sum_xx += x * x;
    }

    let denominator = n * sum_xx - sum_x * sum_x;
    if denominator == 0.0 { return None; }

    let m = (n * sum_xy - sum_x * sum_y) / denominator;
    let b = (sum_y - m * sum_x) / n;

    Some((m, b))
}