//! Audio Paging & Ducking Playbooks.
//!
//! Rules mapping emergency overrides, crowd density, and ambient noise to
//! DuckingProfile curves (-18dB/-20dB over 300ms, or instant full mute) and
//! multi-zone audio routing.

use mediamaster_audio::ducking::{DuckingCurve, DuckingProfile};
use serde::{Deserialize, Serialize};

use crate::error::RagError;

/// Trigger patterns triggering paging or acoustic transitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TriggerPattern {
    /// Life safety / building emergency override.
    EmergencyOverride,
    /// High-priority RF ISM paging clicker / radio burst.
    PriorityRfPaging,
    /// Intercom call / customer service alert.
    IntercomAlert,
    /// Routine ambient scene change.
    AmbientDrift,
}

/// Serializable ducking curve representation for RAG storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RagDuckingCurve {
    Instant,
    Exponential,
    Linear,
}

impl From<DuckingCurve> for RagDuckingCurve {
    fn from(c: DuckingCurve) -> Self {
        match c {
            DuckingCurve::Instant => Self::Instant,
            DuckingCurve::Exponential => Self::Exponential,
            DuckingCurve::Linear => Self::Linear,
        }
    }
}

impl From<RagDuckingCurve> for DuckingCurve {
    fn from(c: RagDuckingCurve) -> Self {
        match c {
            RagDuckingCurve::Instant => Self::Instant,
            RagDuckingCurve::Exponential => Self::Exponential,
            RagDuckingCurve::Linear => Self::Linear,
        }
    }
}

/// Serializable ducking profile configuration for playbooks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlaybookDuckingConfig {
    pub curve: RagDuckingCurve,
    pub duration_ms: u64,
    pub target_gain: f32,
}

impl PlaybookDuckingConfig {
    pub fn new(curve: DuckingCurve, duration_ms: u64, target_gain: f32) -> Self {
        Self {
            curve: curve.into(),
            duration_ms,
            target_gain,
        }
    }

    pub fn to_ducking_profile(&self) -> DuckingProfile {
        DuckingProfile {
            curve: self.curve.into(),
            duration_ms: self.duration_ms,
            target: self.target_gain,
        }
    }
}

/// Crowd and acoustic noise context surrounding the audio zone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AcousticProfile {
    pub crowd_noise_floor_db: f32,
    pub ambient_spl_db: f32,
}

impl AcousticProfile {
    pub fn new(crowd_noise_floor_db: f32, ambient_spl_db: f32) -> Self {
        Self {
            crowd_noise_floor_db,
            ambient_spl_db,
        }
    }

    /// Whether background noise is loud (> 65 dB SPL).
    pub fn is_loud_environment(&self) -> bool {
        self.ambient_spl_db >= 65.0 || self.crowd_noise_floor_db >= -24.0
    }
}

/// Multi-zone ducking and volume routing action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ZoneDuckingRule {
    pub zone_id: String,
    pub ducking_config: PlaybookDuckingConfig,
    pub paging_gain_db: f32,
    pub bypass_mute: bool,
}

/// A complete playbook for an audio ducking/paging event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PagingDuckingPlaybook {
    pub name: String,
    pub trigger: TriggerPattern,
    /// Default ducking configuration applied across the main mix.
    pub default_ducking: PlaybookDuckingConfig,
    pub hold_time_ms: u64,
    pub zone_rules: Vec<ZoneDuckingRule>,
    pub explanation: String,
}

/// Store holding extensible playbooks.
#[derive(Debug, Clone)]
pub struct PlaybookStore {
    playbooks: Vec<PagingDuckingPlaybook>,
}

impl PlaybookStore {
    /// Create a playbook store pre-loaded with standard industrial and retail playbooks.
    pub fn new() -> Self {
        let mut store = Self {
            playbooks: Vec::new(),
        };
        store.load_defaults();
        store
    }

    /// Add or update a playbook.
    pub fn register(&mut self, playbook: PagingDuckingPlaybook) {
        if let Some(pos) = self
            .playbooks
            .iter()
            .position(|p| p.trigger == playbook.trigger)
        {
            self.playbooks[pos] = playbook;
        } else {
            self.playbooks.push(playbook);
        }
    }

    /// Query playbook for a given trigger pattern and acoustic noise profile.
    pub fn query_playbook(
        &self,
        trigger: &TriggerPattern,
        acoustic: Option<&AcousticProfile>,
    ) -> Result<PagingDuckingPlaybook, RagError> {
        let playbook = self
            .playbooks
            .iter()
            .find(|p| &p.trigger == trigger)
            .ok_or(RagError::NoMatchFound)?;

        let mut matched = playbook.clone();

        // Dynamically adjust attenuation if high crowd noise requires stronger ducking
        if let Some(ac) = acoustic {
            if ac.is_loud_environment() && matched.trigger != TriggerPattern::EmergencyOverride {
                // If loud crowd, deepen ducking from e.g. -18dB (0.125) to -24dB (0.063) to ensure intelligibility
                matched.default_ducking.target_gain =
                    (matched.default_ducking.target_gain * 0.5).clamp(0.0, 1.0);
                matched.explanation = format!(
                    "{} [Adjusted for high ambient noise: {:.1}dB SPL]",
                    matched.explanation, ac.ambient_spl_db
                );
            }
        }

        Ok(matched)
    }

    fn load_defaults(&mut self) {
        // 1. Emergency Override: Instant full mute (target 0.0) for life-safety compliance
        self.playbooks.push(PagingDuckingPlaybook {
            name: "EmergencyOverrideFullMute".into(),
            trigger: TriggerPattern::EmergencyOverride,
            default_ducking: PlaybookDuckingConfig::new(
                DuckingCurve::Instant,
                100,
                DuckingProfile::FULL_MUTE,
            ),
            hold_time_ms: 3000,
            zone_rules: Vec::new(),
            explanation: "Life safety override: instantaneous full mute across all channels".into(),
        });

        // 2. Priority RF Paging: -20dB exponential ducking over 300ms
        self.playbooks.push(PagingDuckingPlaybook {
            name: "PriorityRfPagingCurve".into(),
            trigger: TriggerPattern::PriorityRfPaging,
            default_ducking: PlaybookDuckingConfig::new(
                DuckingCurve::Exponential,
                300,
                0.1, // -20 dB attenuation
            ),
            hold_time_ms: 600,
            zone_rules: Vec::new(),
            explanation: "Priority RF paging burst: exponential -20dB ducking over 300ms".into(),
        });

        // 3. Intercom Alert: -18dB linear ducking over 400ms
        self.playbooks.push(PagingDuckingPlaybook {
            name: "IntercomAlertCurve".into(),
            trigger: TriggerPattern::IntercomAlert,
            default_ducking: PlaybookDuckingConfig::new(
                DuckingCurve::Linear,
                400,
                0.125, // -18 dB attenuation
            ),
            hold_time_ms: 1000,
            zone_rules: Vec::new(),
            explanation: "Intercom alert: linear -18dB ducking over 400ms".into(),
        });
    }
}

impl Default for PlaybookStore {
    fn default() -> Self {
        Self::new()
    }
}
