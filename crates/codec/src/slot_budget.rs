//! Hardware codec slot budgeting and degradation-tier fallback.
//!
//! Guards against hardware slot exhaustion (the Android MediaCodec
//! `maxSupportedInstances` limit and the equivalent Windows MF MFT instance
//! budget): [`CodecSlotManager`] counts concurrent decoder/encoder permits
//! against a [`CapsMatrix`] budget and refuses to over-allocate. When the
//! budget is spent, the caller walks the [`DegradationTier`] ladder until a
//! satisfiable tier is reached.

use mediamaster_core::{CapsMatrix, DegradationTier};

/// Whether a slot is a hardware decoder or encoder permit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SlotKind {
    Decoder,
    Encoder,
}

/// A slot could not be acquired because the hardware budget is spent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("no free {kind:?} slot (active {active}/{max})")]
pub struct SlotExhausted {
    pub kind: SlotKind,
    pub active: u8,
    pub max: u8,
}

/// Tracks concurrent hardware decoder/encoder permits against a [`CapsMatrix`]
/// budget (`max_hw_decoder_instances` / `max_hw_encoder_instances`).
///
/// A budget of `0` in the matrix means *unknown*; the manager then treats the
/// budget as unbounded (never exhausts) rather than refusing all hardware —
/// an unknown limit should not silently disable decoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodecSlotManager {
    max_decoders: u8,
    max_encoders: u8,
    active_decoders: u8,
    active_encoders: u8,
}

impl CodecSlotManager {
    /// Build a manager from a host's capability matrix.
    pub fn new(matrix: &CapsMatrix) -> Self {
        Self {
            max_decoders: matrix.max_hw_decoder_instances,
            max_encoders: matrix.max_hw_encoder_instances,
            active_decoders: 0,
            active_encoders: 0,
        }
    }

    /// Build a manager with explicit budgets (`0` = unknown/unbounded).
    pub fn with_budget(decoders: u8, encoders: u8) -> Self {
        Self {
            max_decoders: decoders,
            max_encoders: encoders,
            active_decoders: 0,
            active_encoders: 0,
        }
    }

    fn max(&self, kind: SlotKind) -> u8 {
        match kind {
            SlotKind::Decoder => self.max_decoders,
            SlotKind::Encoder => self.max_encoders,
        }
    }

    fn active(&self, kind: SlotKind) -> u8 {
        match kind {
            SlotKind::Decoder => self.active_decoders,
            SlotKind::Encoder => self.active_encoders,
        }
    }

    fn active_mut(&mut self, kind: SlotKind) -> &mut u8 {
        match kind {
            SlotKind::Decoder => &mut self.active_decoders,
            SlotKind::Encoder => &mut self.active_encoders,
        }
    }

    /// Number of concurrently held permits of `kind`.
    pub fn active_count(&self, kind: SlotKind) -> u8 {
        self.active(kind)
    }

    /// Remaining permits of `kind` (`u8::MAX`-ish for an unknown/`0` budget).
    pub fn available(&self, kind: SlotKind) -> u8 {
        let max = self.max(kind);
        if max == 0 {
            // Unknown budget: report "plenty" (u8::MAX) so callers don't degrade.
            u8::MAX
        } else {
            max.saturating_sub(self.active(kind))
        }
    }

    /// `true` when one more permit of `kind` can be acquired right now.
    pub fn can_acquire(&self, kind: SlotKind) -> bool {
        self.available(kind) > 0
    }

    /// Acquire one permit of `kind`, or fail with [`SlotExhausted`].
    pub fn acquire(&mut self, kind: SlotKind) -> Result<(), SlotExhausted> {
        let max = self.max(kind);
        let active = self.active(kind);
        if max != 0 && active >= max {
            return Err(SlotExhausted { kind, active, max });
        }
        *self.active_mut(kind) = active.saturating_add(1);
        Ok(())
    }

    /// Release one permit of `kind` (floor at `0`).
    pub fn release(&mut self, kind: SlotKind) {
        let active = self.active(kind);
        *self.active_mut(kind) = active.saturating_sub(1);
    }
}

/// The next (lower) tier on the degradation ladder, or `None` at the bottom.
///
/// Tier 1 (hardware surface) → Tier 2 (byte buffer) → Tier 3 (shared texture
/// mux) → Tier 4 (poster fallback) → `None`.
pub fn next_tier(tier: DegradationTier) -> Option<DegradationTier> {
    match tier {
        DegradationTier::Tier1HwSurface => Some(DegradationTier::Tier2HwByteBuffer),
        DegradationTier::Tier2HwByteBuffer => Some(DegradationTier::Tier3SharedTextureMux),
        DegradationTier::Tier3SharedTextureMux => Some(DegradationTier::Tier4PosterFallback),
        DegradationTier::Tier4PosterFallback => None,
    }
}

/// Walk the degradation ladder from `start` until `probe` accepts a tier (or
/// the ladder bottom is reached). Returns the first acceptable tier, or `None`
/// if none of them (including the start tier) is acceptable.
///
/// This is the spec's degradation ladder: when hardware can't meet the demand,
/// step down Tier1 → Tier4 rather than failing outright.
pub fn degrade_until(
    start: DegradationTier,
    mut probe: impl FnMut(DegradationTier) -> bool,
) -> Option<DegradationTier> {
    let mut tier = Some(start);
    while let Some(t) = tier {
        if probe(t) {
            return Some(t);
        }
        tier = next_tier(t);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use mediamaster_core::Codec;
    use mediamaster_core::CodecCaps;
    use mediamaster_core::PixelFormat;

    fn matrix_with_budget(decoders: u8, encoders: u8) -> CapsMatrix {
        CapsMatrix {
            codecs: vec![CodecCaps {
                codec: Codec::H264,
                profile: "High".into(),
                level: "4.1".into(),
                max_width: 1920,
                max_height: 1080,
                max_fps: 60,
                pixel_formats: vec![PixelFormat::Nv12],
                hw_accelerated: true,
            }],
            max_hw_decoder_instances: decoders,
            max_hw_encoder_instances: encoders,
            verified_on_hardware: true,
        }
    }

    #[test]
    fn acquires_up_to_budget_then_exhausts() {
        let mut m = CodecSlotManager::new(&matrix_with_budget(2, 1));
        assert!(m.acquire(SlotKind::Decoder).is_ok());
        assert!(m.acquire(SlotKind::Decoder).is_ok());
        let err = m.acquire(SlotKind::Decoder).unwrap_err();
        assert_eq!(err.active, 2);
        assert_eq!(err.max, 2);
        // Encoder budget is separate.
        assert!(m.acquire(SlotKind::Encoder).is_ok());
        assert!(m.acquire(SlotKind::Encoder).is_err());
    }

    #[test]
    fn release_frees_a_slot() {
        let mut m = CodecSlotManager::new(&matrix_with_budget(1, 0));
        m.acquire(SlotKind::Decoder).unwrap();
        assert!(m.acquire(SlotKind::Decoder).is_err());
        m.release(SlotKind::Decoder);
        assert_eq!(m.active_count(SlotKind::Decoder), 0);
        assert!(m.acquire(SlotKind::Decoder).is_ok());
    }

    #[test]
    fn unknown_budget_never_exhausts() {
        // `0` in the matrix = unknown; treat as unbounded, not zero.
        let mut m = CodecSlotManager::new(&matrix_with_budget(0, 0));
        for _ in 0..200 {
            m.acquire(SlotKind::Decoder).unwrap();
        }
        assert_eq!(m.available(SlotKind::Decoder), u8::MAX);
    }

    #[test]
    fn release_never_underflows() {
        let mut m = CodecSlotManager::new(&matrix_with_budget(2, 2));
        m.release(SlotKind::Decoder);
        m.release(SlotKind::Decoder);
        assert_eq!(m.active_count(SlotKind::Decoder), 0);
    }

    #[test]
    fn degradation_ladder_steps_down_to_poster_then_none() {
        assert_eq!(
            next_tier(DegradationTier::Tier1HwSurface),
            Some(DegradationTier::Tier2HwByteBuffer)
        );
        assert_eq!(
            next_tier(DegradationTier::Tier3SharedTextureMux),
            Some(DegradationTier::Tier4PosterFallback)
        );
        assert_eq!(next_tier(DegradationTier::Tier4PosterFallback), None);
    }

    #[test]
    fn degrade_until_finds_first_acceptable_tier() {
        // Tier1/Tier2 unavailable; Tier3 acceptable.
        let chosen = degrade_until(DegradationTier::Tier1HwSurface, |t| {
            matches!(t, DegradationTier::Tier3SharedTextureMux)
        });
        assert_eq!(chosen, Some(DegradationTier::Tier3SharedTextureMux));
    }

    #[test]
    fn degrade_until_returns_none_when_nothing_acceptable() {
        let chosen = degrade_until(DegradationTier::Tier1HwSurface, |_| false);
        assert_eq!(chosen, None);
    }

    #[test]
    fn degrade_until_accepts_start_tier_immediately() {
        let chosen = degrade_until(DegradationTier::Tier1HwSurface, |_| true);
        assert_eq!(chosen, Some(DegradationTier::Tier1HwSurface));
    }
}
