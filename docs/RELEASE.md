# Mediamaster Release Discipline & Procedure

> **Status:** Active Standard
> **Audience:** Workspace maintainers, release engineers, automated release agents

This document defines the strict, reproducible release procedure for all crates in the
`mediamaster-rs` workspace. Because embedders (such as the Windows Tauri desktop player
and Android companion app) rely on these crates, the release pipeline must be mechanically
trustworthy and honor SemVer.

---

## 1. Versioning Rules & SemVer Discipline

1. **Zero-FFmpeg & Platform Independence**: Library crates must never link platform-only native binaries unless explicitly feature-gated.
2. **SemVer Compliance**:
   - Any public trait signature change (e.g. `AsyncSensor::read` → `read_sample`) is a breaking change and requires at least a minor version bump while `<1.0.0` (or major bump if `>=1.0.0`).
   - Adding variants to non-exhaustive public enums (such as `HardwareEvent`) is a breaking change.
   - Internal dependency bounds across workspace crates must specify exact compatible SemVer ranges.
3. **Changelog Maintenance**:
   - Every crate bump must have an associated entry in `CHANGELOG.md` detailing breaking changes, new features, and bug fixes.
4. **Git Tag Naming**:
   - Coordinated workspace releases use the standard tag format: `v<MAJOR>.<MINOR>.<PATCH>` (e.g., `v0.3.0`, `v0.4.0`).
   - If an individual crate is released out of cycle, use `<crate_name>-v<version>` (e.g., `mediamaster-sensors-v0.3.0`).

---

## 2. Publish Order & Dependency Topology

Because crates.io verifies dependencies on upload, publish order must follow the strict dependency DAG from the bottom up. Wait for each crate to become visible on the crates.io sparse index before publishing the next step.

```text
[Tier 1: Core Foundation]
    mediamaster-core (alloc-only, #![no_std], zero tokio)

[Tier 2: Foundation Utilities & Scaffolds]
    mediamaster-bus (depends on core)
    mediamaster-probe (standalone thiserror)
    mediamaster-codec (depends on core)
    mediamaster-frame (depends on core)
    mediamaster-ffi (depends on core)

[Tier 3: Domain Crates]
    mediamaster-audio (depends on core, bus)
    mediamaster-rf (depends on core, bus)
    mediamaster-sensors (depends on core)

[Tier 4: Domain Stores & Facades]
    mediamaster-rag (depends on core, codec, audio)
    mediamaster-bridge (depends on core, bus, audio, sensors, rf)
    mediamaster-engine (depends on core, bus, audio, rf, sensors, probe, codec, frame)

[Tier 5: Top-Level Orchestration Agent]
    mediamaster-agent (depends on core, bus, audio, sensors, rf, codec, rag, bridge)
```

---

## 3. Pre-Flight Verification Gate

Before initiating a release commit or publish action, all automated checks must pass locally and in CI:

```bash
# 1. Formatting
cargo fmt --check

# 2. Strict linting across all workspace targets
cargo clippy --workspace --all-targets -- -D warnings

# 3. Unit and integration tests
cargo test --workspace

# Hardware Device Validation & Quirks Corpus Seeding (Real Device Pre-Flight)
cargo test -p mediamaster-codec --test device_probe -- --ignored --nocapture

# 4. no_std target check
cargo check -p mediamaster-core --target thumbv7em-none-eabihf --no-default-features
cargo check -p mediamaster-core --target riscv32imac-unknown-none-elf --no-default-features

# 5. Codec platform targets
cargo check -p mediamaster-codec --target x86_64-pc-windows-gnu --features mf
cargo check -p mediamaster-codec --target aarch64-linux-android --features mediacodec
cargo check -p mediamaster-codec --no-default-features

# 6. Feature powerset checks
cargo hack --manifest-path crates/mediamaster-core/Cargo.toml --feature-powerset check
cargo hack --manifest-path crates/mediamaster-audio/Cargo.toml --feature-powerset check
cargo hack --manifest-path crates/mediamaster-probe/Cargo.toml --feature-powerset check
cargo hack --manifest-path crates/mediamaster-rag/Cargo.toml --feature-powerset check
cargo hack --manifest-path crates/mediamaster-sensors/Cargo.toml --feature-powerset check
cargo hack --manifest-path crates/mediamaster-bridge/Cargo.toml --feature-powerset check
cargo hack --manifest-path crates/mediamaster-rf/Cargo.toml --feature-powerset check
```

---

## 4. Execution Step-by-Step

### Step 1: Dry Run Verification
Execute `cargo publish --dry-run` for each crate in top-down dependency order:
```bash
cargo publish --dry-run -p mediamaster-core
cargo publish --dry-run -p mediamaster-bus
cargo publish --dry-run -p mediamaster-probe
cargo publish --dry-run -p mediamaster-codec
cargo publish --dry-run -p mediamaster-audio
cargo publish --dry-run -p mediamaster-rf
cargo publish --dry-run -p mediamaster-sensors
cargo publish --dry-run -p mediamaster-rag
cargo publish --dry-run -p mediamaster-engine
cargo publish --dry-run -p mediamaster-agent
cargo publish --dry-run -p mediamaster-bridge
```

### Step 2: Publish Execution
Execute `cargo publish -p <crate>` following the exact order from Section 2, verifying crates.io index availability between steps.

### Step 3: Git Tagging
Create an annotated tag for the release commit:
```bash
git tag -a v0.4.0 -m "Release mediamaster-rs v0.4.0"
git push origin v0.4.0
```

---

## 5. Post-Publish Smoke Testing

Immediately after publishing, run the scratch project smoke test script to verify that external downstream consumers can build and link against the newly published crates from a clean environment without workspace inheritance:

## 8. After the release

- Move `CHANGELOG.md`'s `[Unreleased]` heading into a `## [0.3.0] — <date>` section.
- Confirm the README `Crates.io` table/badges reflect the new versions.
- `masteraudio-rs.md` §6 Phase 1.5 → ✅ done.

## 9. 0.3 → 0.4 Transition Notes & Breaking Changes
- `AsyncSensor` relocation: `mediamaster-core::traits::AsyncSensor` is deprecated in 0.3.x and will be removed in 0.4.0. Consumers must migrate to `mediamaster-sensors::AsyncSensor`.
- `mediamaster-sensors` bump: `mediamaster-sensors` introduces `SimulatedSensorStream` and uniform `AsyncSensor` implementations across hardware backends (`hw.rs`).
- `mediamaster-agent` pipeline activation: `FusionOutputDriver` and TOML-loadable `AutomationRule` DSL provide active sensor-to-mixer/zones control.
