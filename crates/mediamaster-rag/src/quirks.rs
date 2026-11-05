//! Quirks Corpus & Known Hardware Driver Overlays.
//!
//! Loads and merges TOML/JSON corpora containing observed driver quirks,
//! capability refutations, and empirical degradation limits.
//!
//! # Privacy and Redaction Law
//! When sharing corpora across nodes or exporting telemetry to fleets:
//! - **NO Serial Numbers**: Hardware is identified solely by `device_class` (e.g. SoC / GPU family hash).
//! - **NO Network Identifiers**: IPs, MAC addresses, hostnames, and node IDs must never be exported.
//! - **Quantized Timestamps**: Individual microsecond event timestamps are dropped or quantized into windows.
//! - **Stateless Aggregation**: Only observed capability metrics, verdicts, and confidence ratings are shared.

use mediamaster_core::caps::{CapabilityVerdict, Codec, CodecCaps, ObservedCodecCaps};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

use crate::error::RagError;

/// An individual quirk rule or observed capability overlay for a specific codec on a device class.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuirksEntry {
    /// SoC / GPU family identifier (e.g. "amlogic-s905x4", "rockchip-rk3588", "intel-uhd-770").
    pub device_class: String,
    /// Codec this quirk applies to.
    pub codec: Codec,
    /// Maximum safe width before starvation or crash (if restricted).
    pub max_width: Option<u32>,
    /// Maximum safe height before starvation or crash (if restricted).
    pub max_height: Option<u32>,
    /// Maximum safe FPS under multi-stream load.
    pub max_fps: Option<u32>,
    /// Maximum verified concurrent streams before pipeline collapse.
    pub max_concurrent_streams: Option<u32>,
    /// Whether hardware acceleration is actually functional or a lie.
    pub hw_accelerated: Option<bool>,
    /// Capability verdict based on empirical observation.
    pub verdict: CapabilityVerdict,
    /// Confidence penalty [0.0, 1.0] to deduct from advertised capability confidence.
    #[serde(default)]
    pub confidence_penalty: f32,
    /// Explanatory note or observed failure mode.
    #[serde(default)]
    pub notes: String,
}

impl QuirksEntry {
    /// Unique compound key for a quirk entry: `(device_class, codec)`.
    pub fn key(&self) -> (String, Codec) {
        (self.device_class.to_lowercase(), self.codec)
    }

    /// Commutatively and idempotently merges two quirk entries for the same key.
    ///
    /// Merge rules:
    /// - Dimensions/limits take the conservative minimum (lower is safer).
    /// - `hw_accelerated`: false takes precedence if refuted.
    /// - Verdict precedence: `Refuted` > `Borderline` > `Confirmed`.
    /// - `confidence_penalty`: takes `max(self, other)`.
    pub fn merge_with(&mut self, other: &Self) {
        if self.key() != other.key() {
            return;
        }

        self.max_width = match (self.max_width, other.max_width) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };

        self.max_height = match (self.max_height, other.max_height) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };

        self.max_fps = match (self.max_fps, other.max_fps) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };

        self.max_concurrent_streams =
            match (self.max_concurrent_streams, other.max_concurrent_streams) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };

        self.hw_accelerated = match (self.hw_accelerated, other.hw_accelerated) {
            (Some(false), _) | (_, Some(false)) => Some(false),
            (Some(true), Some(true)) => Some(true),
            (a, b) => a.or(b),
        };

        self.verdict = match (self.verdict, other.verdict) {
            (CapabilityVerdict::Refuted, _) | (_, CapabilityVerdict::Refuted) => {
                CapabilityVerdict::Refuted
            }
            (CapabilityVerdict::Borderline, _) | (_, CapabilityVerdict::Borderline) => {
                CapabilityVerdict::Borderline
            }
            _ => CapabilityVerdict::Confirmed,
        };

        self.confidence_penalty = self.confidence_penalty.max(other.confidence_penalty);

        if self.notes.is_empty() {
            self.notes = other.notes.clone();
        } else if !other.notes.is_empty() && self.notes != other.notes {
            // Sort merged notes to ensure commutative merge result
            let mut parts: Vec<&str> = self
                .notes
                .split(" | ")
                .chain(other.notes.split(" | "))
                .collect();
            parts.sort_unstable();
            parts.dedup();
            self.notes = parts.join(" | ");
        }
    }
}

/// Container for deserializing TOML corpus schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuirksDocument {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub quirks: Vec<QuirksEntry>,
}

/// Redacted JSON export container for sharing empirical observations across fleets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RedactedCorpusExport {
    pub schema_version: String,
    /// Explicit confirmation that all entries adhere to privacy laws.
    pub redaction_law: String,
    pub entries: Vec<QuirksEntry>,
}

/// Corpus of known hardware quirks and empirical capability observations.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct QuirksCorpus {
    entries: BTreeMap<(String, Codec), QuirksEntry>,
}

impl QuirksCorpus {
    /// Creates a new, empty quirks corpus.
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    /// Number of distinct quirk rules in the corpus.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the corpus has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Inserts or merges an entry into the corpus.
    pub fn insert(&mut self, entry: QuirksEntry) {
        let key = entry.key();
        self.entries
            .entry(key)
            .and_modify(|existing| existing.merge_with(&entry))
            .or_insert(entry);
    }

    /// Returns a reference to all entries.
    pub fn entries(&self) -> impl Iterator<Item = &QuirksEntry> {
        self.entries.values()
    }

    /// Loads a corpus from a TOML string.
    pub fn load_toml(toml_str: &str) -> Result<Self, RagError> {
        let doc: QuirksDocument = toml::from_str(toml_str)
            .map_err(|e| RagError::FormatError(format!("Failed to parse TOML quirks: {e}")))?;

        let mut corpus = Self::new();
        for entry in doc.quirks {
            corpus.insert(entry);
        }
        Ok(corpus)
    }

    /// Loads a corpus from a TOML file path.
    pub fn load_toml_file(path: impl AsRef<Path>) -> Result<Self, RagError> {
        let content = std::fs::read_to_string(path.as_ref()).map_err(|e| {
            RagError::IoError(format!("Failed to read {}: {e}", path.as_ref().display()))
        })?;
        Self::load_toml(&content)
    }

    /// Commutative and idempotent merge of another corpus into `self`.
    ///
    /// For any two corpora A and B:
    /// - `A.merge(B)` results in identical entries to `B.merge(A)`
    /// - `A.merge(A)` is an identity operation.
    pub fn merge(&mut self, other: Self) {
        for (_, entry) in other.entries {
            self.insert(entry);
        }
    }

    /// Query the corpus for a `device_class` and an advertised `CodecCaps`.
    ///
    /// Returns an adjusted `CodecCaps` if a quirk or empirical downgrade applies,
    /// or `None` if the hardware behaves normally without known quirks.
    pub fn query(&self, device_class: &str, request: &CodecCaps) -> Option<CodecCaps> {
        let key = (device_class.to_lowercase(), request.codec);
        let quirk = self.entries.get(&key)?;

        let mut adjusted = request.clone();
        let mut modified = false;

        if let Some(w) = quirk.max_width {
            if adjusted.max_width > w {
                adjusted.max_width = w;
                modified = true;
            }
        }

        if let Some(h) = quirk.max_height {
            if adjusted.max_height > h {
                adjusted.max_height = h;
                modified = true;
            }
        }

        if let Some(fps) = quirk.max_fps {
            if adjusted.max_fps > fps {
                adjusted.max_fps = fps;
                modified = true;
            }
        }

        if let Some(hw) = quirk.hw_accelerated {
            if adjusted.hw_accelerated != hw {
                adjusted.hw_accelerated = hw;
                modified = true;
            }
        }

        if quirk.verdict == CapabilityVerdict::Refuted {
            // Refuted capability: demote from hardware acceleration
            if adjusted.hw_accelerated {
                adjusted.hw_accelerated = false;
                modified = true;
            }
        }

        if modified {
            Some(adjusted)
        } else {
            None
        }
    }

    /// Convert an `ObservedCodecCaps` into a `QuirksEntry` for corpus assimilation.
    pub fn record_observation(&mut self, obs: &ObservedCodecCaps) {
        let confidence_penalty = if obs.verdict == CapabilityVerdict::Refuted {
            0.8
        } else if obs.verdict == CapabilityVerdict::Borderline {
            0.4
        } else {
            0.0
        };

        let (max_w, max_h, max_fps, hw) = match obs.verdict {
            CapabilityVerdict::Refuted => {
                // If refuted, drop resolution / fps to standard safe fallback (1080p60) and drop hw_accel
                (
                    Some(obs.max_width.min(1920)),
                    Some(obs.max_height.min(1080)),
                    Some(obs.max_fps.min(60)),
                    Some(false),
                )
            }
            CapabilityVerdict::Borderline => (
                Some(obs.max_width),
                Some(obs.max_height),
                Some(obs.max_fps),
                None,
            ),
            CapabilityVerdict::Confirmed => (
                Some(obs.max_width),
                Some(obs.max_height),
                Some(obs.max_fps),
                Some(true),
            ),
        };

        let entry = QuirksEntry {
            device_class: obs.device_class.clone(),
            codec: obs.codec,
            max_width: max_w,
            max_height: max_h,
            max_fps,
            max_concurrent_streams: Some(obs.streams_concurrently_observed),
            hw_accelerated: hw,
            verdict: obs.verdict,
            confidence_penalty,
            notes: format!(
                "Runtime observation ({} starvation events observed)",
                obs.starvation_events
            ),
        };

        self.insert(entry);
    }

    /// Exports corpus to a redacted JSON string conforming to fleet privacy rules.
    pub fn export_json(&self) -> Result<String, RagError> {
        let export = RedactedCorpusExport {
            schema_version: "1.0".into(),
            redaction_law: "Zero PII, zero serial numbers, zero network endpoints; device_class hash and capability bounds only".into(),
            entries: self.entries.values().cloned().collect(),
        };

        serde_json::to_string_pretty(&export)
            .map_err(|e| RagError::FormatError(format!("Failed to export JSON: {e}")))
    }

    /// Imports a redacted JSON corpus string.
    pub fn import_json(json_str: &str) -> Result<Self, RagError> {
        let export: RedactedCorpusExport = serde_json::from_str(json_str)
            .map_err(|e| RagError::FormatError(format!("Failed to import JSON: {e}")))?;

        let mut corpus = Self::new();
        for entry in export.entries {
            corpus.insert(entry);
        }
        Ok(corpus)
    }
}
