//! Provisioning: device capability bundles + hardware maintenance leases.

use alloc::string::String;
use alloc::vec::Vec;

#[cfg(feature = "serde-sync")]
use serde::{Deserialize, Serialize};

use crate::caps::CodecCaps;
use crate::types::AudioTarget;

/// Stable identifier for a fleet node (desktop, edge, or Android player).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
pub struct NodeId(pub String);

/// Which kind of host a node is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
pub enum HostPlatform {
    WindowsDesktop,
    LinuxEdge,
    AndroidMobile,
}

/// A registered audio output endpoint reported by a node.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
pub struct AudioTargetDescriptor {
    pub label: String,
    pub device: Option<String>,
    pub priority: u8,
}

impl From<AudioTarget> for AudioTargetDescriptor {
    fn from(t: AudioTarget) -> Self {
        Self {
            label: t.label,
            device: t.device,
            priority: t.priority,
        }
    }
}

/// A node's probed media capability bundle, registered with the desktop agent
/// so it can pre-transcode assets to match the node's exact hardware decode
/// profile (and respect `max_hw_decoder_instances`).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
pub struct MediaCapabilityBundleV1 {
    pub schema_version: String,
    pub node_id: NodeId,
    pub platform: HostPlatform,
    /// Concurrent hardware decoder instances (Android slot-exhaustion guard).
    pub max_hw_decoder_instances: u8,
    pub verified_decoders: Vec<CodecCaps>,
    pub verified_encoders: Vec<CodecCaps>,
    pub audio_outputs: Vec<AudioTargetDescriptor>,
    pub probed_at_unix_ms: u64,
}

impl MediaCapabilityBundleV1 {
    pub fn schema_version() -> &'static str {
        "1.0"
    }
}

/// An exclusive claim on a hardware codec slot (or a maintenance window) for a
/// bounded duration, so two subsystems never fight over a decoder/encoder.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
pub struct MaintenanceLease {
    pub holder: NodeId,
    /// Monotonic millisecond expiry.
    pub expires_at_ms: u64,
    pub resource: String,
}

impl MaintenanceLease {
    pub fn is_expired(&self, now_ms: u64) -> bool {
        now_ms >= self.expires_at_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lease_expiry() {
        let lease = MaintenanceLease {
            holder: NodeId("kiosk-1".into()),
            expires_at_ms: 1_000,
            resource: "hw-decoder-0".into(),
        };
        assert!(!lease.is_expired(999));
        assert!(lease.is_expired(1_000));
    }

    #[test]
    fn bundle_schema_version() {
        assert_eq!(MediaCapabilityBundleV1::schema_version(), "1.0");
    }
}
