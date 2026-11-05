use mediamaster_bus::EventBus;
use mediamaster_codec::{CapsDiscovery, QuirkRule, QuirksAwareDiscovery, SlotKind};
use mediamaster_core::caps::{CapsMatrix, Codec, CodecCaps, PixelFormat};
use mediamaster_core::provisioning::HostPlatform;
use mediamaster_engine::CapabilitySnapshotProvider;
use std::sync::Arc;

#[cfg(feature = "attest")]
use mediamaster_engine::verify_bundle_attestation;

struct MockHardwareDiscovery;

impl CapsDiscovery for MockHardwareDiscovery {
    fn discover(&self) -> CapsMatrix {
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
            max_hw_decoder_instances: 2,
            max_hw_encoder_instances: 1,
            verified_on_hardware: true,
        }
    }
}

#[tokio::test]
async fn bundle_reflects_slot_exhaustion_after_revocations() {
    let bus = Arc::new(EventBus::new(16));
    let mut rx = bus.subscribe();

    let quirks_disco = QuirksAwareDiscovery::new(MockHardwareDiscovery);
    let provider = CapabilitySnapshotProvider::new(
        "kiosk-node-42",
        HostPlatform::WindowsDesktop,
        quirks_disco,
    )
    .with_event_bus(bus);

    let initial = provider.snapshot();
    assert_eq!(initial.max_hw_decoder_instances, 2);
    assert_eq!(provider.available_decoders(), 2);

    // Acquire first slot
    assert!(provider.acquire_slot(SlotKind::Decoder).is_ok());
    assert_eq!(provider.available_decoders(), 1);
    let event1 = rx.recv().await.expect("bus event on slot acquire");
    assert_eq!(event1.event_name(), "ZoneChanged");

    let snapshot_one = provider.snapshot();
    assert_eq!(snapshot_one.max_hw_decoder_instances, 1);

    // Acquire second slot -> slot budget exhausted
    assert!(provider.acquire_slot(SlotKind::Decoder).is_ok());
    assert_eq!(provider.available_decoders(), 0);
    let event2 = rx.recv().await.expect("bus event on slot acquire");
    assert_eq!(event2.event_name(), "ZoneChanged");

    let snapshot_exhausted = provider.snapshot();
    assert_eq!(snapshot_exhausted.max_hw_decoder_instances, 0);

    // Further acquisition fails
    assert!(provider.acquire_slot(SlotKind::Decoder).is_err());

    // Release slot -> recovers
    provider.release_slot(SlotKind::Decoder);
    assert_eq!(provider.available_decoders(), 1);
    let snapshot_recovered = provider.snapshot();
    assert_eq!(snapshot_recovered.max_hw_decoder_instances, 1);
}

#[test]
fn bundle_reflects_quirks_override() {
    let mut quirks_disco = QuirksAwareDiscovery::new(MockHardwareDiscovery);
    quirks_disco.add_quirk(QuirkRule {
        codec: Codec::H264,
        disable_hw: true,
        max_hw_decoder_override: Some(1),
    });

    let provider =
        CapabilitySnapshotProvider::new("kiosk-quirked", HostPlatform::AndroidMobile, quirks_disco);

    let snap = provider.snapshot();
    assert_eq!(snap.max_hw_decoder_instances, 1);
    assert!(!snap.verified_decoders[0].hw_accelerated);
}

#[cfg(feature = "attest")]
#[test]
fn signed_snapshot_attestation_verifies() {
    use ed25519_dalek::SigningKey;

    let secret_bytes: [u8; 32] = [42u8; 32];
    let signing_key = SigningKey::from_bytes(&secret_bytes);
    let quirks_disco = QuirksAwareDiscovery::new(MockHardwareDiscovery);

    let provider =
        CapabilitySnapshotProvider::new("kiosk-signed", HostPlatform::LinuxEdge, quirks_disco)
            .with_signing_key(signing_key);

    let signed_snapshot = provider.signed_snapshot().expect("signed snapshot");
    assert!(signed_snapshot.signature_hex.is_some());
    assert!(signed_snapshot.public_key_hex.is_some());

    let is_valid = verify_bundle_attestation(&signed_snapshot).expect("verification");
    assert!(is_valid, "attestation signature failed verification");

    // Test tamper resistance
    let mut tampered = signed_snapshot.clone();
    tampered.bundle.node_id = mediamaster_core::provisioning::NodeId("tampered-node".into());
    let tamper_result = verify_bundle_attestation(&tampered);
    assert!(
        tamper_result.is_err() || !tamper_result.unwrap(),
        "tampered bundle must not verify"
    );
}
