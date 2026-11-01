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

/// A lock-free automation command for the real-time gain path.
///
/// The async plane pushes these through a `ringbuf::HeapRb<AutomationCommand>`;
/// the render thread drains and applies them with branchless single-pole IIR
/// smoothing (`gain += (target - gain) * coeff`) — no mutex, no heap, no await.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AutomationCommand {
    /// Ramp the gain toward `target_gain` using pole coefficient `coeff`.
    DuckStart { target_gain: f32, coeff: f32 },
    /// Release back toward full volume using pole coefficient `coeff`.
    DuckRelease { coeff: f32 },
}

impl AutomationCommand {
    /// Single-pole IIR coefficient to settle within ~`duration_ms` at
    /// `sample_rate` (branchless, computed on the async plane).
    ///
    /// `coeff = 1 - e^(-k/(duration*sr))`; a smaller `coeff` = slower ease.
    pub fn coeff_for(duration_ms: u32, sample_rate: u32) -> f32 {
        let duration_ms = duration_ms.max(1) as f64;
        let sr = sample_rate.max(1) as f64;
        let samples = duration_ms * sr / 1000.0;
        (1.0 - (-1.0 / samples).exp()) as f32
    }

    /// The target gain this command settles at.
    pub fn target(&self, full_volume: f32) -> f32 {
        match *self {
            Self::DuckStart { target_gain, .. } => target_gain,
            Self::DuckRelease { .. } => full_volume,
        }
    }

    /// The pole coefficient for this command.
    pub fn coeff(&self) -> f32 {
        match *self {
            Self::DuckStart { coeff, .. } => coeff,
            Self::DuckRelease { coeff } => coeff,
        }
    }
}

/// Branchless single-pole IIR gain smoother (per-sample, allocation-free).
#[derive(Debug, Clone, Copy)]
pub struct IirSmoothing {
    pub current: f32,
    pub target: f32,
    pub coeff: f32,
    pub active: bool,
}

impl IirSmoothing {
    pub fn new() -> Self {
        Self {
            current: 1.0,
            target: 1.0,
            coeff: 1.0,
            active: false,
        }
    }

    /// Apply an automation command.
    pub fn apply(&mut self, cmd: AutomationCommand) {
        self.target = cmd.target(DuckingProfile::FULL_VOLUME);
        self.coeff = cmd.coeff().clamp(0.0, 1.0);
        self.active = true;
    }

    /// Advance one sample: `current += (target - current) * coeff`.
    #[inline]
    pub fn step(&mut self) -> f32 {
        if self.active {
            self.current += (self.target - self.current) * self.coeff;
        }
        self.current.clamp(0.0, 1.0)
    }
}

impl Default for IirSmoothing {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automation_coeff_shrinks_with_longer_duration() {
        let fast = AutomationCommand::coeff_for(50, 48_000);
        let slow = AutomationCommand::coeff_for(500, 48_000);
        assert!(fast > slow, "shorter ease needs a larger pole: {fast} <= {slow}");
        assert!(fast > 0.0 && fast <= 1.0);
        assert!(slow > 0.0 && slow <= 1.0);
    }

    #[test]
    fn automation_accessors() {
        let duck = AutomationCommand::DuckStart { target_gain: 0.3, coeff: 0.02 };
        assert_eq!(duck.target(1.0), 0.3);
        assert_eq!(duck.coeff(), 0.02);
        let rel = AutomationCommand::DuckRelease { coeff: 0.01 };
        assert_eq!(rel.target(1.0), 1.0);
        assert_eq!(rel.coeff(), 0.01);
    }

    #[test]
    fn iir_eases_to_duck_target_branchless_and_monotone() {
        let mut iir = IirSmoothing::new();
        assert!(!iir.active);
        assert_eq!(iir.step(), 1.0, "inactive smoother stays at unity");

        iir.apply(AutomationCommand::DuckStart { target_gain: 0.0, coeff: 0.05 });
        assert!(iir.active);
        let mut prev = 1.0;
        for _ in 0..500 {
            let g = iir.step();
            assert!(g <= prev + 1e-6, "ease must be monotone non-increasing: {g} > {prev}");
            assert!((0.0..=1.0).contains(&g));
            prev = g;
        }
        assert!(iir.current < 0.05, "did not ease to duck target: {}", iir.current);
    }

    #[test]
    fn iir_release_rises_to_full_volume() {
        let mut iir = IirSmoothing::new();
        iir.apply(AutomationCommand::DuckStart { target_gain: 0.0, coeff: 0.05 });
        for _ in 0..500 {
            iir.step();
        }
        assert!(iir.current < 0.5, "should be ducked before release: {}", iir.current);
        iir.apply(AutomationCommand::DuckRelease { coeff: 0.05 });
        for _ in 0..500 {
            iir.step();
        }
        assert!((iir.current - 1.0).abs() < 0.05, "did not release to full: {}", iir.current);
    }

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
