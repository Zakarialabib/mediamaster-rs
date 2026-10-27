//! Smoothly interpolated ducking profiles.

/// A ducking curve describing how background audio scales over time when a
/// high-priority event fires.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DuckingCurve {
    /// Instant step down.
    Instant,
    /// Smooth exponential decay.
    Exponential,
    /// Linear ramp.
    Linear,
}

/// A ducking profile with a duration and target level.
#[derive(Debug, Clone)]
pub struct DuckingProfile {
    pub curve: DuckingCurve,
    pub duration_ms: u64,
    pub target: f32,
}

impl DuckingProfile {
    pub const FULL_MUTE: f32 = 0.0;
    pub const FULL_VOLUME: f32 = 1.0;

    /// Interpolate the ducking level at `t` (milliseconds since the request).
    pub fn level(&self, t_ms: u64) -> f32 {
        let t = t_ms as f32;
        let dur = self.duration_ms as f32;
        if t <= 0.0 {
            return 1.0;
        }
        if t >= dur {
            return self.target;
        }
        match self.curve {
            DuckingCurve::Instant => {
                if t < dur / 2.0 {
                    1.0
                } else {
                    self.target
                }
            }
            DuckingCurve::Linear => 1.0 - (1.0 - self.target) * (t / dur),
            DuckingCurve::Exponential => {
                1.0 - (1.0 - self.target) * (1.0 - (-t / (dur / 2.0).max(1.0)))
            }
        }
    }
}

impl Default for DuckingProfile {
    fn default() -> Self {
        Self {
            curve: DuckingCurve::Exponential,
            duration_ms: 300,
            target: Self::FULL_MUTE,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_interpolation() {
        let profile = DuckingProfile {
            curve: DuckingCurve::Linear,
            duration_ms: 100,
            target: 0.0,
        };
        assert!((profile.level(0) - 1.0).abs() < 1e-6);
        assert!((profile.level(50) - 0.5).abs() < 1e-6);
        assert!((profile.level(100) - 0.0).abs() < 1e-6);
        assert!((profile.level(150) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn instant_curve_steps_at_midpoint() {
        let profile = DuckingProfile {
            curve: DuckingCurve::Instant,
            duration_ms: 100,
            target: 0.0,
        };
        assert!((profile.level(0) - 1.0).abs() < 1e-6);
        assert!((profile.level(50) - 0.0).abs() < 1e-6);
        assert!((profile.level(100) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn exponential_curve_reaches_target() {
        let profile = DuckingProfile {
            curve: DuckingCurve::Exponential,
            duration_ms: 100,
            target: 0.0,
        };
        assert!((profile.level(0) - 1.0).abs() < 1e-6);
        assert!(profile.level(100) <= 1e-6);
        assert!((profile.level(150) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn default_is_exponential_300ms_full_mute() {
        let profile = DuckingProfile::default();
        assert_eq!(profile.curve, DuckingCurve::Exponential);
        assert_eq!(profile.duration_ms, 300);
        assert_eq!(profile.target, DuckingProfile::FULL_MUTE);
    }
}
