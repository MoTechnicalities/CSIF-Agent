use std::f64::consts::PI;

#[inline]
pub fn wrap_pi(theta: f64) -> f64 {
    ((theta + PI).rem_euclid(2.0 * PI)) - PI
}

#[inline]
pub fn phase_distance(a: f64, b: f64) -> f64 {
    wrap_pi(a - b).abs()
}

#[inline]
pub fn normalized_resonance(query_phase: f64, memory_phase: f64) -> f64 {
    phase_distance(query_phase, memory_phase) / PI
}

#[inline]
pub fn contradiction_threshold(sigma: f64, c: f64) -> f64 {
    (PI / 2.0) + c * sigma
}

#[inline]
pub fn temporal_wave_phase(theta_0: f64, sigma: f64, t: f64) -> f64 {
    wrap_pi(theta_0 + sigma * (0.618_f64 * t).sin())
}

pub fn circular_mean(phases: &[f64]) -> Option<f64> {
    if phases.is_empty() {
        return None;
    }

    let (sum_sin, sum_cos) = phases.iter().fold((0.0_f64, 0.0_f64), |acc, p| {
        (acc.0 + p.sin(), acc.1 + p.cos())
    });

    Some(wrap_pi(sum_sin.atan2(sum_cos)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    #[test]
    fn wrap_bounds() {
        let x = wrap_pi(3.0 * PI);
        assert!(x >= -PI && x <= PI);
    }

    #[test]
    fn replay_is_deterministic() {
        let a = temporal_wave_phase(0.0, 0.75, 260.14);
        let b = temporal_wave_phase(0.0, 0.75, 260.14);
        assert_eq!(a.to_bits(), b.to_bits());
    }
}
