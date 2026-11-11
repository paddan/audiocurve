//! Band centre frequencies and crossover points.

pub const BAND_COUNT: usize = 12;

pub const CENTERS: [f64; BAND_COUNT] = [
    125.0, 250.0, 500.0, 750.0, 1000.0, 1500.0, 2000.0, 3000.0, 4000.0, 6000.0, 8000.0, 12000.0,
];

/// Crossover frequencies at the geometric mean of adjacent centres.
pub fn crossovers() -> [f64; BAND_COUNT - 1] {
    let mut x = [0.0; BAND_COUNT - 1];
    for (i, v) in x.iter_mut().enumerate() {
        *v = (CENTERS[i] * CENTERS[i + 1]).sqrt();
    }
    x
}
