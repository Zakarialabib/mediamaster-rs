#!/usr/bin/env bash
set -euo pipefail

# test-smoke.sh
# Verifies that an external embedder / consumer can compile and link against the
# workspace crates without workspace path inheritance or internal root config.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
TMP_DIR="$(mktemp -d -t mediamaster-smoke-XXXXXX)"

cleanup() {
    rm -rf "$TMP_DIR"
}
trap cleanup EXIT

echo "==> Creating scratch smoke test project at $TMP_DIR..."

cp "$WORKSPACE_ROOT/rust-toolchain.toml" "$TMP_DIR/rust-toolchain.toml"

cat << 'EOF' > "$TMP_DIR/Cargo.toml"
[package]
name = "smoke-consumer"
version = "0.1.0"
edition = "2021"

[dependencies]
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
mediamaster-core = { version = "0.3.0" }
mediamaster-bus = { version = "0.3.0" }
mediamaster-codec = { version = "0.3.0" }
mediamaster-audio = { version = "0.3.0" }
mediamaster-probe = { version = "0.3.0" }
mediamaster-rag = { version = "0.1.0" }
mediamaster-sensors = { version = "0.3.0" }
mediamaster-rf = { version = "0.3.0" }
mediamaster-bridge = { version = "0.2.0" }

[patch.crates-io]
# In post-publish CI or local validation before crates.io index propagation,
# this points to the workspace crates.
mediamaster-core = { path = "WORKSPACE_ROOT/crates/mediamaster-core" }
mediamaster-bus = { path = "WORKSPACE_ROOT/crates/mediamaster-bus" }
mediamaster-codec = { path = "WORKSPACE_ROOT/crates/mediamaster-codec" }
mediamaster-audio = { path = "WORKSPACE_ROOT/crates/mediamaster-audio" }
mediamaster-probe = { path = "WORKSPACE_ROOT/crates/mediamaster-probe" }
mediamaster-rag = { path = "WORKSPACE_ROOT/crates/mediamaster-rag" }
mediamaster-sensors = { path = "WORKSPACE_ROOT/crates/mediamaster-sensors" }
mediamaster-rf = { path = "WORKSPACE_ROOT/crates/mediamaster-rf" }
mediamaster-bridge = { path = "WORKSPACE_ROOT/crates/mediamaster-bridge" }
EOF

# Substitute WORKSPACE_ROOT
sed -i "s|WORKSPACE_ROOT|$WORKSPACE_ROOT|g" "$TMP_DIR/Cargo.toml"

mkdir -p "$TMP_DIR/src"
cat << 'EOF' > "$TMP_DIR/src/main.rs"
use std::sync::Arc;
use mediamaster_core::traits::AudioSource;
use mediamaster_core::events::HardwareEvent;
use mediamaster_core::types::RoomState;
use mediamaster_bus::EventBus;
use mediamaster_codec::slot_budget::CodecSlotManager;
use mediamaster_audio::mixer::Mixer;
use mediamaster_bridge::device::{DeviceRegistry, DeviceRegistryImpl};
use mediamaster_bridge::audio::AudioBridge;
use mediamaster_bridge::events::EventHandlers;
use mediamaster_bridge::sensors::SensorBridge;
use mediamaster_bridge::rf::RfBridge;

#[tokio::main]
async fn main() {
    println!("Testing smoke consumer linking...");

    // 1. Core traits & Bus
    let bus = Arc::new(EventBus::new(16));
    let _rx = bus.subscribe();

    // 2. Audio Bridge & Mixer
    let audio = Arc::new(AudioBridge::new());
    assert_eq!(audio.label(), "mediamaster-bridge audio");
    let mixer = Mixer::new();
    assert_eq!(mixer.ducking(), 1.0);

    // 3. Sensor & RF Bridges
    let _sensor_bridge = SensorBridge::new();
    let _rf_bridge = RfBridge::new();

    // 4. Device Registry
    let registry = DeviceRegistryImpl::new();
    let devices = registry.enumerate_devices();
    println!("Enumerate devices: {:?}", devices.len());

    // 5. Event Handlers
    let handlers = EventHandlers::new(audio.clone());
    handlers.handle_event(&HardwareEvent::SensorContextChange {
        state: RoomState::Empty,
    });
    assert_eq!(handlers.last_room_state(), Some(RoomState::Empty));

    // 6. Codec Slot Manager
    let slot_mgr = CodecSlotManager::with_budget(2, 1);
    assert!(slot_mgr.can_acquire(mediamaster_codec::slot_budget::SlotKind::Decoder));

    println!("All mediamaster crates linked successfully in scratch consumer!");
}
EOF

echo "==> Building and running scratch smoke test project..."
cd "$TMP_DIR"
cargo check --quiet
cargo test --quiet
cargo run --quiet

echo "==> Scratch project smoke test passed successfully!"
