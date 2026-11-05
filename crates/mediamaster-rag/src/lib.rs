//! `mediamaster-rag` — knowledge store for codec capabilities,
//! environmental scene memory, and acoustic paging playbooks.

pub mod codec_corpus;
pub mod discovery;
pub mod engine;
pub mod error;
pub mod playbooks;
pub mod quirks;
pub mod scene_memory;
pub mod telemetry_ingest;

pub use codec_corpus::{
    CodecCorpusEntry, CodecKnowledgeStore, CodecQuery, CodecRecommendation, DegradationAction,
    HardwareFingerprint, StarvationTelemetry, StreamDescriptor, StreamLayout,
};
pub use discovery::QuirksAwareDiscovery;
pub use engine::{MediamasterRagFacade, RagContextEnvelope};
pub use error::RagError;
pub use playbooks::{
    AcousticProfile, DuckingCurve, DuckingProfile, PagingDuckingPlaybook, PlaybookStore,
    TriggerPattern, ZoneDuckingRule,
};
pub use quirks::{QuirksCorpus, QuirksDocument, QuirksEntry, RedactedCorpusExport};
pub use scene_memory::{
    AtmospherePlaybook, OptimalAudioState, SceneMemoryStore, SceneQuery, SceneRecord, SensorVector,
    TemporalMetadata,
};
pub use telemetry_ingest::TelemetryIngest;
