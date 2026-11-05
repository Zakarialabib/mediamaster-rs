//! Media capability snapshot service for cast target negotiation.
//!
//! A companion device (e.g. mobile remote or signage controller) must know
//! **before** casting whether a target display host can decode a proposed media
//! stream without stalling or exhausting hardware codec permits.
//!
//! [`CapabilitySnapshotProvider`] composes:
//! 1. Host [`CapsMatrix`] (from [`CapsDiscovery`]);
//! 2. Concurrent hardware instance availability from [`CodecSlotManager`];
//! 3. Driver quirks and telemetry overrides via [`QuirksAwareDiscovery`];
//!
//! into a point-in-time [`MediaCapabilityBundleV1`]. Changes to codec permits
//! or audio endpoints trigger automatic broadcast over [`mediamaster_bus::EventBus`].
//!
//! Under the `attest` feature, snapshots can be cryptographically signed with
//! a detached Ed25519 signature to prove hardware provenance and tamper resistance.

use mediamaster_bus::EventBus;
use mediamaster_codec::{CapsDiscovery, CodecSlotManager, QuirksAwareDiscovery, SlotKind};
use mediamaster_core::events::HardwareEvent;
use mediamaster_core::provisioning::{
    AudioTargetDescriptor, HostPlatform, MediaCapabilityBundleV1, NodeId,
};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(feature = "attest")]
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};

#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    #[error("serialization error: {0}")]
    Serialization(String),
    #[error("signature error: {0}")]
    Signature(String),
}

/// A media capability bundle snapshot alongside an optional detached Ed25519 signature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SignedCapabilitySnapshot {
    pub bundle: MediaCapabilityBundleV1,
    /// Detached 64-byte Ed25519 signature (hex-encoded) when signed.
    pub signature_hex: Option<String>,
    /// Verifying public key (32-byte hex-encoded) when signed.
    pub public_key_hex: Option<String>,
}

/// Inner state of the snapshot provider.
struct ProviderState<D: CapsDiscovery> {
    node_id: NodeId,
    platform: HostPlatform,
    discovery: QuirksAwareDiscovery<D>,
    slot_manager: CodecSlotManager,
    audio_outputs: Vec<AudioTargetDescriptor>,
}

/// Service that generates current [`MediaCapabilityBundleV1`] snapshots on demand
/// and publishes notifications when capability/slot states change.
pub struct CapabilitySnapshotProvider<D: CapsDiscovery> {
    state: Mutex<ProviderState<D>>,
    bus: Option<Arc<EventBus>>,
    #[cfg(feature = "attest")]
    signing_key: Option<SigningKey>,
}

impl<D: CapsDiscovery> CapabilitySnapshotProvider<D> {
    /// Create a new snapshot provider for a node.
    pub fn new(
        node_id: impl Into<String>,
        platform: HostPlatform,
        discovery: QuirksAwareDiscovery<D>,
    ) -> Self {
        let initial_matrix = discovery.discover();
        let slot_manager = CodecSlotManager::new(&initial_matrix);
        Self {
            state: Mutex::new(ProviderState {
                node_id: NodeId(node_id.into()),
                platform,
                discovery,
                slot_manager,
                audio_outputs: Vec::new(),
            }),
            bus: None,
            #[cfg(feature = "attest")]
            signing_key: None,
        }
    }

    /// Attach an [`EventBus`] to publish snapshot change notifications.
    pub fn with_event_bus(mut self, bus: Arc<EventBus>) -> Self {
        self.bus = Some(bus);
        self
    }

    /// Configure an Ed25519 signing key for cryptographic attestation.
    #[cfg(feature = "attest")]
    pub fn with_signing_key(mut self, signing_key: SigningKey) -> Self {
        self.signing_key = Some(signing_key);
        self
    }

    /// Register or update audio output endpoints.
    pub fn set_audio_outputs(&self, outputs: Vec<AudioTargetDescriptor>) {
        if let Ok(mut state) = self.state.lock() {
            state.audio_outputs = outputs;
        }
        self.notify_change();
    }

    /// Acquire a hardware codec permit, updating available slots in future snapshots.
    pub fn acquire_slot(&self, kind: SlotKind) -> Result<(), mediamaster_codec::SlotExhausted> {
        let res = if let Ok(mut state) = self.state.lock() {
            state.slot_manager.acquire(kind)
        } else {
            Ok(())
        };
        if res.is_ok() {
            self.notify_change();
        }
        res
    }

    /// Release a hardware codec permit.
    pub fn release_slot(&self, kind: SlotKind) {
        if let Ok(mut state) = self.state.lock() {
            state.slot_manager.release(kind);
        }
        self.notify_change();
    }

    /// Number of remaining available hardware decoder slots.
    pub fn available_decoders(&self) -> u8 {
        self.state
            .lock()
            .map(|s| s.slot_manager.available(SlotKind::Decoder))
            .unwrap_or(0)
    }

    /// Generate a fresh point-in-time [`MediaCapabilityBundleV1`].
    pub fn snapshot(&self) -> MediaCapabilityBundleV1 {
        let state = self.state.lock().expect("lock state");
        let matrix = state.discovery.discover();

        let available_dec = state.slot_manager.available(SlotKind::Decoder);
        // If max_hw_decoder_instances was specified (> 0), cap it by available slots
        let effective_hw_decoders = if matrix.max_hw_decoder_instances == 0 {
            0
        } else {
            available_dec.min(matrix.max_hw_decoder_instances)
        };

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        MediaCapabilityBundleV1 {
            schema_version: MediaCapabilityBundleV1::schema_version().into(),
            node_id: state.node_id.clone(),
            platform: state.platform,
            max_hw_decoder_instances: effective_hw_decoders,
            verified_decoders: matrix.codecs.clone(),
            verified_encoders: Vec::new(),
            audio_outputs: state.audio_outputs.clone(),
            probed_at_unix_ms: now_ms,
        }
    }

    /// Generate a [`SignedCapabilitySnapshot`], optionally with detached Ed25519 signature.
    pub fn signed_snapshot(&self) -> Result<SignedCapabilitySnapshot, SnapshotError> {
        let bundle = self.snapshot();

        #[cfg(feature = "attest")]
        if let Some(signing_key) = &self.signing_key {
            let serialized = serde_json::to_vec(&bundle)
                .map_err(|e| SnapshotError::Serialization(e.to_string()))?;
            let sig = signing_key.sign(&serialized);
            let verifying_key: VerifyingKey = signing_key.verifying_key();

            return Ok(SignedCapabilitySnapshot {
                bundle,
                signature_hex: Some(hex_encode(&sig.to_bytes())),
                public_key_hex: Some(hex_encode(verifying_key.as_bytes())),
            });
        }

        Ok(SignedCapabilitySnapshot {
            bundle,
            signature_hex: None,
            public_key_hex: None,
        })
    }

    /// Notify subscribers on the bus that a capability or slot state change occurred.
    fn notify_change(&self) {
        if let Some(bus) = &self.bus {
            // Broadcast capability change event
            let _ = bus.publish(HardwareEvent::ZoneChanged {
                zone_ids: vec!["caps-change".into()],
                volume_db: 0.0,
                muted: false,
            });
        }
    }
}

/// Helper to verify an Ed25519 signature on a `MediaCapabilityBundleV1`.
#[cfg(feature = "attest")]
pub fn verify_bundle_attestation(
    snapshot: &SignedCapabilitySnapshot,
) -> Result<bool, SnapshotError> {
    let (sig_hex, pk_hex) = match (&snapshot.signature_hex, &snapshot.public_key_hex) {
        (Some(s), Some(pk)) => (s, pk),
        _ => return Ok(false),
    };

    let sig_bytes = hex_decode(sig_hex).map_err(SnapshotError::Signature)?;
    let pk_bytes = hex_decode(pk_hex).map_err(SnapshotError::Signature)?;

    if sig_bytes.len() != 64 || pk_bytes.len() != 32 {
        return Err(SnapshotError::Signature(
            "invalid key/signature length".into(),
        ));
    }

    let mut pk_fixed = [0u8; 32];
    pk_fixed.copy_from_slice(&pk_bytes);
    let verifying_key =
        VerifyingKey::from_bytes(&pk_fixed).map_err(|e| SnapshotError::Signature(e.to_string()))?;

    let mut sig_fixed = [0u8; 64];
    sig_fixed.copy_from_slice(&sig_bytes);
    let signature = Signature::from_bytes(&sig_fixed);

    let serialized = serde_json::to_vec(&snapshot.bundle)
        .map_err(|e| SnapshotError::Serialization(e.to_string()))?;

    verifying_key
        .verify(&serialized, &signature)
        .map(|_| true)
        .map_err(|e| SnapshotError::Signature(e.to_string()))
}

#[cfg(feature = "attest")]
fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

#[cfg(feature = "attest")]
fn hex_decode(s: &str) -> Result<Vec<u8>, String> {
    if s.len() % 2 != 0 {
        return Err("odd hex string length".into());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| e.to_string()))
        .collect()
}
