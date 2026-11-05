//! Hardware codec slot budgeting and degradation-tier fallback.
//!
//! Guards against hardware slot exhaustion (the Android MediaCodec
//! `maxSupportedInstances` limit and the equivalent Windows MF MFT instance
//! budget): [`CodecSlotManager`] counts concurrent decoder/encoder permits
//! against a [`mediamaster_core::CapsMatrix`] budget and refuses to over-allocate. When the
//! budget is spent, the caller walks the [`DegradationTier`] ladder until a
//! satisfiable tier is reached.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use mediamaster_core::caps::{CapabilityVerdict, Codec, ObservedCodecCaps};
use mediamaster_core::events::{EventPublisher, HardwareEvent};
use mediamaster_core::{CapsMatrix, DegradationTier};

/// Key for tracking failure counts per codec configuration.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CodecFailureKey {
    pub backend: String,
    pub codec: Codec,
    pub width: u32,
    pub height: u32,
}

/// Backend interface for probing the health of active codec leases.
pub trait CodecBackend: Send + Sync {
    /// Validates whether an active lease remains healthy on the underlying device.
    /// Returns `true` if healthy, or `false` (or error reason) if device failed / lost.
    fn validate_lease(&self, lease: &SlotLease) -> Result<(), String>;
}

/// A health probe hook for validating leases.
pub trait HealthProbe {
    /// Probe all active leases against `backend`. Revokes failing leases and emits `SlotRevoked`
    /// via `publisher`.
    fn probe_health(
        &mut self,
        backend: &impl CodecBackend,
        publisher: Option<&impl EventPublisher>,
    ) -> Vec<(u64, String)>;
}

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

/// Information recorded for an active lease.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaseInfo {
    pub slot_id: u64,
    pub kind: SlotKind,
    pub owner_tag: String,
}

/// Inner state shared between `CodecSlotManager` and `SlotLease`.
#[derive(Debug)]
pub(crate) struct SlotState {
    pub(crate) matrix: CapsMatrix,
    pub(crate) max_decoders: u8,
    pub(crate) max_encoders: u8,
    pub(crate) active_decoders: u8,
    pub(crate) active_encoders: u8,
    pub(crate) leases: BTreeMap<u64, LeaseInfo>,
    pub(crate) consecutive_failures: BTreeMap<CodecFailureKey, u32>,
    pub(crate) observed_caps: Vec<ObservedCodecCaps>,
    pub(crate) failure_threshold: u32,
}

impl SlotState {
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

    fn release_slot(&mut self, slot_id: u64, kind: SlotKind) -> bool {
        if self.leases.remove(&slot_id).is_some() {
            let active = self.active(kind);
            *self.active_mut(kind) = active.saturating_sub(1);
            true
        } else {
            false
        }
    }
}

/// A lease representing an acquired hardware codec slot.
///
/// Automatically releases the slot upon `Drop`, and also supports explicit idempotent release.
#[derive(Debug)]
pub struct SlotLease {
    pub slot_id: u64,
    pub kind: SlotKind,
    pub owner_tag: String,
    state: Option<Arc<Mutex<SlotState>>>,
    released: AtomicBool,
}

impl SlotLease {
    /// Create a new SlotLease.
    pub(crate) fn new(
        slot_id: u64,
        kind: SlotKind,
        owner_tag: String,
        state: Arc<Mutex<SlotState>>,
    ) -> Self {
        Self {
            slot_id,
            kind,
            owner_tag,
            state: Some(state),
            released: AtomicBool::new(false),
        }
    }

    /// Whether this lease is currently active (not explicitly released or dropped).
    pub fn is_active(&self) -> bool {
        !self.released.load(Ordering::Acquire)
    }

    /// Explicitly release the slot permit back to the manager (idempotent).
    pub fn release(&self) -> bool {
        if !self.released.swap(true, Ordering::AcqRel) {
            if let Some(state_arc) = &self.state {
                if let Ok(mut state) = state_arc.lock() {
                    return state.release_slot(self.slot_id, self.kind);
                }
            }
        }
        false
    }
}

impl Drop for SlotLease {
    fn drop(&mut self) {
        self.release();
    }
}

/// Tracks concurrent hardware decoder/encoder permits against a [`mediamaster_core::CapsMatrix`]
/// budget (`max_hw_decoder_instances` / `max_hw_encoder_instances`).
///
/// A budget of `0` in the matrix means *unknown*; the manager then treats the
/// budget as unbounded (never exhausts) rather than refusing all hardware —
/// an unknown limit should not silently disable decoding.
#[derive(Debug, Clone)]
pub struct CodecSlotManager {
    state: Arc<Mutex<SlotState>>,
    next_slot_id: Arc<AtomicU64>,
}

impl CodecSlotManager {
    /// Build a manager from a host's capability matrix.
    pub fn new(matrix: &CapsMatrix) -> Self {
        Self {
            state: Arc::new(Mutex::new(SlotState {
                matrix: matrix.clone(),
                max_decoders: matrix.max_hw_decoder_instances,
                max_encoders: matrix.max_hw_encoder_instances,
                active_decoders: 0,
                active_encoders: 0,
                leases: BTreeMap::new(),
                consecutive_failures: BTreeMap::new(),
                observed_caps: Vec::new(),
                failure_threshold: 3,
            })),
            next_slot_id: Arc::new(AtomicU64::new(1)),
        }
    }

    /// Build a manager with explicit budgets (`0` = unknown/unbounded).
    pub fn with_budget(decoders: u8, encoders: u8) -> Self {
        Self {
            state: Arc::new(Mutex::new(SlotState {
                matrix: CapsMatrix {
                    max_hw_decoder_instances: decoders,
                    max_hw_encoder_instances: encoders,
                    ..CapsMatrix::default()
                },
                max_decoders: decoders,
                max_encoders: encoders,
                active_decoders: 0,
                active_encoders: 0,
                leases: BTreeMap::new(),
                consecutive_failures: BTreeMap::new(),
                observed_caps: Vec::new(),
                failure_threshold: 3,
            })),
            next_slot_id: Arc::new(AtomicU64::new(1)),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, SlotState> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Number of concurrently held permits of `kind`.
    pub fn active_count(&self, kind: SlotKind) -> u8 {
        self.lock().active(kind)
    }

    /// Remaining permits of `kind` (`u8::MAX`-ish for an unknown/`0` budget).
    pub fn available(&self, kind: SlotKind) -> u8 {
        let state = self.lock();
        let max = state.max(kind);
        if max == 0 {
            // Unknown budget: report "plenty" (u8::MAX) so callers don't degrade.
            u8::MAX
        } else {
            max.saturating_sub(state.active(kind))
        }
    }

    /// `true` when one more permit of `kind` can be acquired right now.
    pub fn can_acquire(&self, kind: SlotKind) -> bool {
        self.available(kind) > 0
    }

    /// Acquire one permit lease of `kind` with an owner tag, or fail with [`SlotExhausted`].
    pub fn acquire_lease(
        &mut self,
        kind: SlotKind,
        owner_tag: impl Into<String>,
    ) -> Result<SlotLease, SlotExhausted> {
        let mut state = self.lock();
        let max = state.max(kind);
        let active = state.active(kind);
        if max != 0 && active >= max {
            return Err(SlotExhausted { kind, active, max });
        }
        *state.active_mut(kind) = active.saturating_add(1);
        let slot_id = self.next_slot_id.fetch_add(1, Ordering::Relaxed);
        let tag = owner_tag.into();
        state.leases.insert(
            slot_id,
            LeaseInfo {
                slot_id,
                kind,
                owner_tag: tag.clone(),
            },
        );
        Ok(SlotLease::new(slot_id, kind, tag, Arc::clone(&self.state)))
    }

    /// Acquire one permit of `kind`, or fail with [`SlotExhausted`].
    pub fn acquire(&mut self, kind: SlotKind) -> Result<(), SlotExhausted> {
        let mut state = self.lock();
        let max = state.max(kind);
        let active = state.active(kind);
        if max != 0 && active >= max {
            return Err(SlotExhausted { kind, active, max });
        }
        *state.active_mut(kind) = active.saturating_add(1);
        Ok(())
    }

    /// Release one permit of `kind` (floor at `0`).
    /// If an active lease exists, removes one lease, otherwise decrements active count.
    pub fn release(&mut self, kind: SlotKind) {
        let mut state = self.lock();
        // If there are tracked leases for this kind, remove the first one.
        let to_remove = state
            .leases
            .iter()
            .find(|(_, info)| info.kind == kind)
            .map(|(&id, _)| id);
        if let Some(id) = to_remove {
            state.release_slot(id, kind);
        } else {
            let active = state.active(kind);
            *state.active_mut(kind) = active.saturating_sub(1);
        }
    }

    /// Current number of active leases tracked.
    pub fn lease_count(&self) -> usize {
        self.lock().leases.len()
    }

    /// Revoke a specific lease by slot_id with a reason, releasing its budget and returning its info.
    pub fn revoke_lease(&mut self, slot_id: u64, _reason: &str) -> Option<LeaseInfo> {
        let mut state = self.lock();
        if let Some(info) = state.leases.remove(&slot_id) {
            let active = state.active(info.kind);
            *state.active_mut(info.kind) = active.saturating_sub(1);
            Some(info)
        } else {
            None
        }
    }

    /// Set the consecutive failure threshold after which observed caps are persisted and clamped.
    pub fn set_failure_threshold(&mut self, threshold: u32) {
        self.lock().failure_threshold = threshold.max(1);
    }

    /// Record a failure for a specific (backend, codec, width, height) tuple.
    /// When consecutive failures reach `failure_threshold`, records `ObservedCodecCaps`
    /// and clamps advertised caps in the manager.
    pub fn record_failure(
        &mut self,
        backend: impl Into<String>,
        codec: Codec,
        width: u32,
        height: u32,
    ) -> Option<ObservedCodecCaps> {
        let backend_str = backend.into();
        let key = CodecFailureKey {
            backend: backend_str.clone(),
            codec,
            width,
            height,
        };

        let mut state = self.lock();
        let count = state.consecutive_failures.entry(key.clone()).or_insert(0);
        *count = count.saturating_add(1);
        let failure_count = *count;
        let threshold = state.failure_threshold;

        if failure_count >= threshold {
            // Determine clamped instances: if we failed, clamp max instances to below the active level or decrease
            let current_max = state.max_decoders;
            let clamped_instances = state
                .active_decoders
                .saturating_sub(1)
                .min(current_max.saturating_sub(1));

            // Epistemic truth: update state max and clamp CapsMatrix
            state.max_decoders = clamped_instances;
            state.matrix.max_hw_decoder_instances = clamped_instances;
            state.matrix.verified_on_hardware = true;

            // Also clamp matching codec in CapsMatrix if resolution is exceeded
            if let Some(c) = state.matrix.codecs.iter_mut().find(|c| c.codec == codec) {
                if width <= c.max_width && height <= c.max_height {
                    c.max_width = width.saturating_sub(1).max(1280);
                    c.max_height = height.saturating_sub(1).max(720);
                }
            }

            let observed = ObservedCodecCaps {
                device_class: backend_str,
                codec,
                profile: String::new(),
                level: String::new(),
                max_width: width,
                max_height: height,
                max_fps: 60,
                streams_concurrently_observed: clamped_instances as u32,
                starvation_events: failure_count,
                verdict: CapabilityVerdict::Refuted,
                confidence: 1.0,
                observed_at_start_ms: 0,
                observed_at_end_ms: 0,
            };

            state.observed_caps.push(observed.clone());
            Some(observed)
        } else {
            None
        }
    }

    /// Reset consecutive failure counter on success.
    pub fn record_success(
        &mut self,
        backend: impl Into<String>,
        codec: Codec,
        width: u32,
        height: u32,
    ) {
        let key = CodecFailureKey {
            backend: backend.into(),
            codec,
            width,
            height,
        };
        self.lock().consecutive_failures.remove(&key);
    }

    /// All observed capabilities recorded by the manager.
    pub fn observed_caps(&self) -> Vec<ObservedCodecCaps> {
        self.lock().observed_caps.clone()
    }

    /// Current advertised capability matrix (reflects clamped caps).
    pub fn caps_matrix(&self) -> CapsMatrix {
        self.lock().matrix.clone()
    }
}

impl HealthProbe for CodecSlotManager {
    fn probe_health(
        &mut self,
        backend: &impl CodecBackend,
        publisher: Option<&impl EventPublisher>,
    ) -> Vec<(u64, String)> {
        // Collect current lease snapshots to probe
        let current_leases: Vec<LeaseInfo> = {
            let state = self.lock();
            state.leases.values().cloned().collect()
        };

        let mut revoked = Vec::new();

        for lease_info in current_leases {
            // Construct a temporary lease handle for validation
            let temp_lease = SlotLease {
                slot_id: lease_info.slot_id,
                kind: lease_info.kind,
                owner_tag: lease_info.owner_tag.clone(),
                state: None,
                released: AtomicBool::new(false),
            };

            if let Err(reason) = backend.validate_lease(&temp_lease) {
                // Revoke this lease
                if self.revoke_lease(lease_info.slot_id, &reason).is_some() {
                    if let Some(publ) = publisher {
                        publ.publish(HardwareEvent::SlotRevoked {
                            device_class: lease_info.owner_tag.clone(),
                            codec: Codec::H264,
                            reason: reason.clone(),
                        });
                    }
                    revoked.push((lease_info.slot_id, reason));
                }
            }
        }

        revoked
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
    fn explicit_release_is_idempotent() {
        let mut m = CodecSlotManager::new(&matrix_with_budget(2, 0));
        let l1 = m.acquire_lease(SlotKind::Decoder, "test").unwrap();
        assert_eq!(m.active_count(SlotKind::Decoder), 1);
        assert!(l1.release());
        assert_eq!(m.active_count(SlotKind::Decoder), 0);
        assert!(!l1.release()); // idempotent: subsequent releases return false
        drop(l1);
        assert_eq!(m.active_count(SlotKind::Decoder), 0);
    }

    #[test]
    fn lease_drop_releases_slot() {
        let mut m = CodecSlotManager::new(&matrix_with_budget(1, 0));
        {
            let _l = m.acquire_lease(SlotKind::Decoder, "scoped").unwrap();
            assert_eq!(m.active_count(SlotKind::Decoder), 1);
        }
        assert_eq!(m.active_count(SlotKind::Decoder), 0);
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

    struct MockBackend {
        failing_slots: Mutex<std::collections::HashSet<u64>>,
    }

    impl CodecBackend for MockBackend {
        fn validate_lease(&self, lease: &SlotLease) -> Result<(), String> {
            let failing = self.failing_slots.lock().unwrap();
            if failing.contains(&lease.slot_id) {
                Err(format!(
                    "Device lost or GPU hung for slot {}",
                    lease.slot_id
                ))
            } else {
                Ok(())
            }
        }
    }

    struct MockPublisher {
        events: Mutex<Vec<HardwareEvent>>,
    }

    impl EventPublisher for MockPublisher {
        fn publish(&self, event: HardwareEvent) {
            self.events.lock().unwrap().push(event);
        }
    }

    #[test]
    fn health_probe_revokes_failing_leases_and_emits_event() {
        let mut m = CodecSlotManager::new(&matrix_with_budget(3, 1));
        let l1 = m.acquire_lease(SlotKind::Decoder, "cam1").unwrap();
        let l2 = m.acquire_lease(SlotKind::Decoder, "cam2").unwrap();
        let _l3 = m.acquire_lease(SlotKind::Decoder, "cam3").unwrap();

        assert_eq!(m.active_count(SlotKind::Decoder), 3);
        assert_eq!(m.lease_count(), 3);

        let backend = MockBackend {
            failing_slots: Mutex::new(std::collections::HashSet::from([l2.slot_id])),
        };
        let publisher = MockPublisher {
            events: Mutex::new(Vec::new()),
        };

        // Probe health: l2 should fail and be revoked
        let revoked = m.probe_health(&backend, Some(&publisher));
        assert_eq!(revoked.len(), 1);
        assert_eq!(revoked[0].0, l2.slot_id);

        // Active decoder count should decrease to 2
        assert_eq!(m.active_count(SlotKind::Decoder), 2);
        assert_eq!(m.lease_count(), 2);

        // SlotRevoked event emitted on publisher
        let events = publisher.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        if let HardwareEvent::SlotRevoked {
            device_class,
            reason,
            ..
        } = &events[0]
        {
            assert_eq!(device_class, "cam2");
            assert!(reason.contains("Device lost"));
        } else {
            panic!("Expected SlotRevoked event");
        }

        // Dropping l1 and l3 releases remaining
        drop(l1);
        drop(_l3);
        assert_eq!(m.active_count(SlotKind::Decoder), 0);
        assert_eq!(m.lease_count(), 0);
    }

    #[test]
    fn lying_driver_consecutive_failures_clamp_caps() {
        // Driver claims 4 decoder instances and 4K support
        let mut m = CodecSlotManager::new(&matrix_with_budget(4, 1));
        m.set_failure_threshold(3);

        assert_eq!(m.caps_matrix().max_hw_decoder_instances, 4);

        // Simulate 2 failures: threshold not met yet
        assert!(m
            .record_failure("rockchip-vpu", Codec::H264, 3840, 2160)
            .is_none());
        assert!(m
            .record_failure("rockchip-vpu", Codec::H264, 3840, 2160)
            .is_none());
        assert_eq!(m.caps_matrix().max_hw_decoder_instances, 4);

        // 3rd consecutive failure: threshold met! Clamps advertised caps and records ObservedCodecCaps
        let observed = m.record_failure("rockchip-vpu", Codec::H264, 3840, 2160);
        assert!(observed.is_some());
        let obs = observed.unwrap();
        assert_eq!(obs.device_class, "rockchip-vpu");
        assert_eq!(obs.verdict, CapabilityVerdict::Refuted);

        // Advertised caps clamped
        assert!(m.caps_matrix().max_hw_decoder_instances < 4);
        assert!(m.caps_matrix().verified_on_hardware);

        // Manager snapshot shows observed caps
        assert_eq!(m.observed_caps().len(), 1);
    }

    #[test]
    fn vanishing_devices_no_lease_leak_after_all_drops() {
        let mut m = CodecSlotManager::new(&matrix_with_budget(2, 2));
        {
            let l1 = m.acquire_lease(SlotKind::Decoder, "usb-webcam-1").unwrap();
            let l2 = m.acquire_lease(SlotKind::Decoder, "usb-webcam-2").unwrap();
            assert_eq!(m.active_count(SlotKind::Decoder), 2);
            assert_eq!(m.lease_count(), 2);

            // Explicitly release l1 early
            assert!(l1.release());
            assert_eq!(m.active_count(SlotKind::Decoder), 1);
            assert_eq!(m.lease_count(), 1);

            // l2 dropped at end of scope
            drop(l2);
        }

        // All leases dropped: assert 0 held
        assert_eq!(m.active_count(SlotKind::Decoder), 0);
        assert_eq!(m.lease_count(), 0);
    }
}
