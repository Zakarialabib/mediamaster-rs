# Companion Cast Negotiation & MediaCapabilityBundle Cookbook

This guide details the cast negotiation flow between a companion device (e.g. Android or iOS mobile remote, signage controller) and a playback target host (e.g. Windows desktop kiosk or embedded Linux player) using `mediamaster-engine`.

## Motivation & Architecture

Before initiating a media cast or push-to-talk stream, the casting companion must discover what codecs, resolutions, and concurrent hardware instances the target host can support without risking decoder starvation or stuttering.

The negotiation pattern uses:
1. **`MediaCapabilityBundleV1`**: Standardized capability manifest specifying platform, supported hardware codecs, concurrent hardware decoder limits, and audio endpoints.
2. **`CapabilitySnapshotProvider`**: The host-side service in `mediamaster-engine` composing:
   - Operating system codec discovery (`CapsDiscovery`)
   - Driver quirks & telemetry overrides (`QuirksAwareDiscovery`)
   - Real-time hardware instance permits (`CodecSlotManager`)
3. **Event Bus Change Notifications**: Automatic updates published over `mediamaster-bus::EventBus` whenever slots are acquired, released, or audio endpoints change.
4. **Attestation (`attest` feature)**: Cryptographic detached Ed25519 signatures ensuring the bundle originated from an authentic device.

---

## Host Implementation Pattern

### 1. Initialize the Provider

The host embedder initializes `CapabilitySnapshotProvider` with its platform identifier and discovery pipeline.

```rust
use std::sync::Arc;
use mediamaster_bus::EventBus;
use mediamaster_codec::{QuirksAwareDiscovery, SoftwareCapsDiscovery, QuirkRule, Codec};
use mediamaster_core::provisioning::HostPlatform;
use mediamaster_engine::CapabilitySnapshotProvider;

// 1. Setup bus and discovery
let bus = Arc::new(EventBus::new(128));
let mut quirks_discovery = QuirksAwareDiscovery::new(SoftwareCapsDiscovery);

// Optionally attach known driver quirk rules
quirks_discovery.add_quirk(QuirkRule {
    codec: Codec::Hevc,
    disable_hw: false,
    max_hw_decoder_override: Some(3),
});

// 2. Instantiate provider and attach bus
let provider = CapabilitySnapshotProvider::new(
    "kiosk-display-01",
    HostPlatform::WindowsDesktop,
    quirks_discovery,
).with_event_bus(bus.clone());
```

### 2. Optional Cryptographic Attestation

Enable the `attest` feature in `mediamaster-engine` to attach an Ed25519 signing key:

```rust
#[cfg(feature = "attest")]
{
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    let signing_key = SigningKey::generate(&mut OsRng);
    let provider = provider.with_signing_key(signing_key);
}
```

### 3. Expose via Embedder Transport (HTTP / SSE / WebSocket)

The transport layer is explicitly left to the host application embedder (e.g. Axum, Actix, or raw TCP):

```rust
// HTTP GET /api/v1/capabilities
async fn get_capabilities(
    provider: Arc<CapabilitySnapshotProvider<SoftwareCapsDiscovery>>
) -> Result<String, Box<dyn std::error::Error>> {
    let signed_snapshot = provider.signed_snapshot()?;
    Ok(serde_json::to_string(&signed_snapshot)?)
}
```

### 4. Codec Slot Management on Stream Playback

When streams begin or end, acquire/release slots through the provider:

```rust
use mediamaster_codec::SlotKind;

// When a video stream starts:
provider.acquire_slot(SlotKind::Decoder)?;

// When playback terminates:
provider.release_slot(SlotKind::Decoder);
```

When permits change, `provider` automatically fires an event over `EventBus`, allowing host SSE/WebSocket streams to broadcast updated capabilities to connected companions.

---

## Companion Verification Pattern

Upon receiving a `SignedCapabilitySnapshot` from the target host, the companion verifies the attestation and inspects available slots:

```rust
use mediamaster_engine::{verify_bundle_attestation, SignedCapabilitySnapshot};

fn verify_and_check_cast(snapshot: &SignedCapabilitySnapshot) -> bool {
    // 1. Verify Ed25519 attestation
    #[cfg(feature = "attest")]
    if let Ok(valid) = verify_bundle_attestation(snapshot) {
        if !valid {
            eprintln!("Rejecting snapshot: invalid signature");
            return false;
        }
    }

    // 2. Inspect available decoder instances
    if snapshot.bundle.max_hw_decoder_instances == 0 {
        // Unknown or exhausted limit
        eprintln!("Target reports 0 available hardware decoder slots");
        return false;
    }

    // 3. Verify codec support (e.g. H264 / HEVC)
    let can_decode_hevc = snapshot.bundle.verified_decoders
        .iter()
        .any(|c| c.codec.as_str() == "hevc" && c.max_height >= 1080);

    can_decode_hevc
}
```
