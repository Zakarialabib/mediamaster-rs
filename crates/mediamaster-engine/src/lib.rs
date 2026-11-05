//! `mediamaster-engine` — engine coordinator: **AudioBus** (lock-free PCM ring) +
//! **TranscodePlan** (capability decision) + **EngineSelector** (backend pick).
//!
//! A coordination layer sitting on top of the 0.3.0 crates. The agent's
//! "Phase 4" live-paging / sensor-ducking loop is the *control* plane; the
//! engine crate is the *coordinate* plane that selects hardware backends and
//! emits transcode plans from a device's `CapsMatrix` (via
//! `mediamaster_codec::CapsMatrix` + `mediamaster_codec::CapsDiscovery`).
//!
//! The crate deliberately **excludes** the native `mf` / `mediacodec` features —
//! those stay device-validation-pending and are only exercised at runtime on
//! target devices. The portable fallback path (`SoftwareCapsDiscovery`) is the
//! default and publishable path.

pub mod audio_bus;
pub mod engine_selector;
#[cfg(feature = "hls")]
pub mod hls;
pub mod poster;
pub mod retention;
pub mod transcode_plan;

pub use audio_bus::{AudioBus, AudioBusConfig};
pub use engine_selector::{Engine, EngineSelector, SoftwareCapsDiscovery};
#[cfg(feature = "hls")]
pub use hls::{
    resolve_rendition_ladder, HlsContainer, HlsSegmenter, HlsSegmenterConfig, InMemoryRollingSink,
    MasterPlaylistEntry, PartRecord, RenditionSpec, ResolvedRendition, Segment, SegmentKind,
    SegmentRecord, SegmentSink, TsMuxer,
};
#[cfg(feature = "hls")]
pub use transcode_plan::HlsJob;
pub use poster::{BackdropSink, EncodedPacket, PacketSource, PosterProducer};
pub use retention::{EvictionPlan, RetainedItem, RetentionEngine};
pub use transcode_plan::{Codec, Container, Profile, TranscodePlan};

pub type EngineError = Box<dyn std::error::Error + Send + Sync>;
