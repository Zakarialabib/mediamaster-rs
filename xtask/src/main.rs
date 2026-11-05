//! Workspace automation for `mediamaster-rs`.
//!
//! Invoked through the `cargo xtask` alias defined in `.cargo/config.toml`:
//!
//! ```text
//! cargo xtask publish [--dry-run] [--crate <name>]   # crates.io release driver
//! cargo xtask hw-test                                # async-plane device smoke test
//! ```
//!
//! `publish` just orchestrates `cargo publish` in the dependency order from
//! `docs/RELEASE.md` §2, so the crates.io index always resolves a crate before
//! its dependents are uploaded. `hw-test` links the integration facade
//! (`mediamaster-bridge`) and exercises the runtime it will run on: a live
//! `EventBus`, the bridge device enumerators, and the `EventHandlers` router.

use std::process::{Command, ExitCode};

use mediamaster_bridge::{AudioBridge, EventHandlers, RfBridge, SensorBridge};
use mediamaster_bus::EventBus;
use mediamaster_core::events::HardwareEvent;
use mediamaster_core::types::RoomState;

/// Workspace crates in coordinated publish order (leaf-first).
///
/// Mirrors `docs/RELEASE.md` §2 — the tiers must be respected because crates.io
/// verifies dependency bounds on upload. `mediamaster-integration-tests` is
/// `publish = false` and therefore absent.
const PUBLISH_ORDER: &[&str] = &[
    // Tier 1 — core foundation
    "mediamaster-core",
    // Tier 2 — foundation utilities & scaffolds
    "mediamaster-bus",
    "mediamaster-probe",
    "mediamaster-codec",
    // Tier 3 — domain crates
    "mediamaster-audio",
    "mediamaster-rf",
    "mediamaster-sensors",
    // Tier 4 — coordination & intelligence
    "mediamaster-rag",
    "mediamaster-engine",
    "mediamaster-agent",
    // Tier 5 — integration facade
    "mediamaster-bridge",
];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.first().map(String::as_str) {
        Some("publish") => publish(&args[1..]),
        Some("hw-test") => hw_test(),
        Some("--help" | "-h") | None => {
            usage();
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("xtask: unknown task `{other}`\n");
            usage();
            ExitCode::FAILURE
        }
    }
}

fn usage() {
    println!(
        "mediamaster-rs xtask\n\n\
         USAGE:\n    \
         cargo xtask publish [--dry-run] [--crate <name>]\n    \
         cargo xtask hw-test\n\n\
         TASKS:\n    \
         publish    Publish workspace crates to crates.io in dependency order\n    \
         hw-test    Exercise the runtime: EventBus, device enumerators, event router"
    );
}

/// Publish every publishable workspace crate in dependency order.
///
/// `--dry-run` maps to `cargo publish --dry-run` (build + package, no upload).
/// `--crate <name>` limits the run to a single crate already in the order.
fn publish(args: &[String]) -> ExitCode {
    let dry_run = args.iter().any(|a| a == "--dry-run");
    let only = args
        .windows(2)
        .find(|w| w[0] == "--crate")
        .map(|w| w[1].as_str());

    let targets: Vec<&str> = match only {
        Some(name) => match PUBLISH_ORDER.iter().find(|c| **c == name).copied() {
            Some(c) => vec![c],
            None => {
                eprintln!("xtask: `{name}` is not a publishable workspace crate");
                return ExitCode::FAILURE;
            }
        },
        None => PUBLISH_ORDER.to_vec(),
    };

    if dry_run {
        println!("xtask: dry run — no crate will be uploaded");
    }

    for name in targets {
        let mut cmd = Command::new("cargo");
        cmd.args(["publish", "-p", name]);
        if dry_run {
            cmd.arg("--dry-run");
        }

        println!(
            "\nxtask: cargo publish -p {name}{}",
            if dry_run { " --dry-run" } else { "" }
        );
        match cmd.status() {
            Ok(status) if status.success() => {}
            Ok(status) => {
                eprintln!("xtask: `cargo publish -p {name}` failed ({status})");
                return ExitCode::FAILURE;
            }
            Err(err) => {
                eprintln!("xtask: could not run cargo: {err}");
                return ExitCode::FAILURE;
            }
        }
    }

    println!(
        "\nxtask: publish complete ({} crate(s))",
        PUBLISH_ORDER.len()
    );
    ExitCode::SUCCESS
}

/// Runtime smoke test: enumerate hardware through the facade and verify the
/// shared async event plane routes a sensor event into the bridge handlers.
#[tokio::main(flavor = "current_thread")]
async fn hw_test() -> ExitCode {
    println!("xtask hw-test — enumerating devices through mediamaster-bridge");

    let audio = AudioBridge::new();
    let sensors = SensorBridge::new();
    let rf = RfBridge::new();

    let internal_audio = audio.enumerate_internal_audio_devices();
    let external_audio = audio.enumerate_external_audio_devices();
    let internal_sensors = sensors.enumerate_internal_sensor_devices();
    let external_sensors = sensors.enumerate_external_sensor_devices();
    let internal_rf = rf.enumerate_internal_rf_devices();
    let external_rf = rf.enumerate_external_rf_devices();

    println!(
        "  audio    : {} internal, {} external",
        internal_audio.len(),
        external_audio.len()
    );
    println!(
        "  sensors  : {} internal, {} external",
        internal_sensors.len(),
        external_sensors.len()
    );
    println!(
        "  rf       : {} internal, {} external",
        internal_rf.len(),
        external_rf.len()
    );

    // Drive one event across the async plane and confirm the router observed it.
    let bus = EventBus::new(32);
    let mut rx = bus.subscribe();
    let handlers = EventHandlers::new(std::sync::Arc::new(AudioBridge::new()));

    let event = HardwareEvent::SensorContextChange {
        state: RoomState::Empty,
    };
    if !bus.publish(event.clone()) {
        eprintln!("xtask: bus dropped the event (no subscriber)");
        return ExitCode::FAILURE;
    }
    handlers.handle_event(&event);

    if tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
        .await
        .is_err()
    {
        eprintln!("xtask: timed out waiting for the bus event");
        return ExitCode::FAILURE;
    }

    if handlers.last_room_state() != Some(RoomState::Empty) {
        eprintln!("xtask: event router did not record the room state");
        return ExitCode::FAILURE;
    }

    println!("xtask: runtime smoke test passed");
    ExitCode::SUCCESS
}
