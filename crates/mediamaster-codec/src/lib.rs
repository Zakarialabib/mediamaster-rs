//! `mediamaster-codec` — capability-aware codec slotting.
//!
//! Consumes the [`mediamaster_core::CapsMatrix`] vocabulary from `mediamaster-core` to manage
//! concurrent hardware codec permits ([`CodecSlotManager`]) and walk the
//! [`mediamaster_core::DegradationTier`] fallback ladder when the hardware budget is spent.
//!
//! Platform-native backends (Windows Media Foundation, Android MediaCodec NDK)
//! plug into [`CapsDiscovery`]; they are feature-gated (`mf`, `mediacodec`) so
//! the core logic stays portable and testable on any host.
//!
//! ```
//! use mediamaster_codec::{CodecSlotManager, SlotKind};
//! use mediamaster_core::{CapsMatrix, DegradationTier};
//!
//! let mut slots = CodecSlotManager::new(&CapsMatrix {
//!     max_hw_decoder_instances: 2,
//!     ..CapsMatrix::default()
//! });
//! let _l1 = slots.acquire(SlotKind::Decoder).expect("first decoder slot");
//! let _l2 = slots.acquire(SlotKind::Decoder).expect("second decoder slot");
//! assert!(slots.acquire(SlotKind::Decoder).is_err()); // budget spent
//! ```

pub mod discovery;
pub mod slot_budget;

pub mod mf_actor;
#[cfg(feature = "mediacodec")]
pub mod ndk_codec;

pub use discovery::{CapsDiscovery, QuirkRule, QuirksAwareDiscovery, SoftwareCapsDiscovery};
pub use mf_actor::FramePacket;
pub use slot_budget::{
    degrade_until, next_tier, CodecBackend, CodecSlotManager, HealthProbe, LeaseInfo,
    SlotExhausted, SlotKind, SlotLease,
};
