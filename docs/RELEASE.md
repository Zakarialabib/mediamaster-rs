# Release plan — mediamaster-rs 0.3.0

> **Status:** plan (not yet executed) · **Audience:** core team
> **Companion:** [`CHANGELOG.md`](../CHANGELOG.md) · [`MEDIA-SCOPE-SPEC.md`](docs/MEDIA-SCOPE-SPEC.md) (§6)

This is the coordinated 0.3.0 expansion publish plan: `mediamaster-probe`
(first deliverable), `mediamaster-codec` (native mediation), and
`mediamaster-agent` (reconciliation loop) plus the purified `mediamaster-core`
(+ CapsMatrix vocabulary). The baseline crates (`audio`, `rf`, `sensors`,
`bridge`) stay at their published versions; cross-dep pins are re-pointed to
the bumped `0.3.0` crates.

## 1. Current state (verified 2026-11-02)

| Crate | Version (local) | Status | `publish` in Cargo.toml |
|-------|------------------|--------|--------------------------|
| `mediamaster-core` | 0.3.0 | purified + CapsMatrix; not yet published to crates.io | default (true) |
| `mediamaster-bus` | 0.3.0 | extracted out of core; not yet published | default (true) |
| `mediamaster-probe` | 0.3.0 (was 0.1.0) | new, ready | default (true) — `publish = false` **removed** |
| `mediamaster-codec` | 0.3.0 (was 0.1.0) | new, ready | default (true) — `publish = false` **removed** |
| `mediamaster-agent` | 0.3.0 (was 0.1.0) | new, ready | default (true) |
| `mediamaster-audio` | 0.2.0 | published (baseline) | default (true) |
| `mediamaster-rf` | 0.2.0 | published (baseline) | default (true) |
| `mediamaster-sensors` | 0.2.0 | published (baseline) | default (true) |
| `mediamaster-bridge` | 0.1.0 | published (baseline) | default (true) |
| `mediamaster-integration-tests` | 0.1.0 | test harness — `publish = false` (not published) | `publish = false` |

> Native codec backends (`mf` = Windows Media Foundation, `mediacodec` =
> Android MediaCodec NDK) stay **device-validation-pending, local-only**. They
> are **not** published as native executables; the `mf`/`mediacodec` Cargo.toml
> features are feature-gated and compile portably. Nothing here runs on this
> host (no Windows/Android toolchains).

## 2. Target versions (aligned 0.3.0)

| Crate | From → To | Why |
|-------|-----------|-----|
| `mediamaster-core` | 0.2.0 → **0.3.0** | purified (tokio stripped, EventBus relocated); CapsMatrix vocab added |
| `mediamaster-bus` | 0.3.0 (extracted) | EventBus removed from core |
| `mediamaster-audio` | 0.2.0 | unchanged baseline (published) |
| `mediamaster-rf` | 0.2.0 | unchanged baseline (published) |
| `mediamaster-sensors` | 0.2.0 | unchanged baseline (published) |
| `mediamaster-bridge` | 0.1.0 | unchanged baseline (published) |
| `mediamaster-probe` | 0.1.0 → **0.3.0** | Phase 1 deliverable (pure-Rust container/bitset probe) |
| `mediamaster-codec` | 0.1.0 → **0.3.0** | Phase 3 portable slice + native MF/NDK scaffolds |
| `mediamaster-agent` | 0.1.0 → **0.3.0** | Phase 4 live paging & sensor ducking loop |

> Semver note: the new crates (`probe`, `codec`, `agent`) are **called out at
> 0.3.0** so they depend on `mediamaster-core` 0.3.0 (no duplicate-type split);
> the baseline crates keep their published 0.2.0/0.1.0 versions.

## 3. Dependency re-pins (required before publishing the dependents)

The new crates (`probe`, `codec`, `agent`) depend on `mediamaster-core` 0.3.0
and `mediamaster-bus` 0.3.0. Baseline crates already pin correctly. No further
re-pins needed for `mediamaster-core`/`mediamaster-bus` (they have no
cross-crate deps).

## 4. Pre-flight (must be green)

```bash
cargo fmt --check
cargo clippy --workspace --no-default-features --all-targets -- -D warnings
cargo test --workspace --no-default-features
```

### Hardware Device Validation & Quirks Corpus Seeding (Real Device Pre-Flight)

Before publishing, execute the ignored device probe harness on physical target
hardware (Windows, Linux, Android) to validate hardware codec limits, verify that
the reported maximum matches reality within tolerance, and generate machine-readable
JSON reports for quirks-corpus seed data:

```bash
cargo test -p mediamaster-codec --test device_probe -- --ignored --nocapture
```

## 5. Publish order (dependency-first, wait for crates.io to index between steps)

```bash
# 1. core (no internal deps besides serde)
cargo publish --dry-run -p mediamaster-core
cargo publish -p mediamaster-core

# 2. bus (tokio + serde)
cargo publish --dry-run -p mediamaster-bus
cargo publish -p mediamaster-bus

# 3. probe (thiserror)
cargo publish --dry-run -p mediamaster-probe
cargo publish -p mediamaster-probe

# 4. codec (core + thiserror + optional windows/ndk)
cargo publish --dry-run -p mediamaster-codec
cargo publish -p mediamaster-codec

# 5. agent (core + bus + audio/rf/sensors/bridge + thiserror + tokio)
cargo publish --dry-run -p mediamaster-agent
cargo publish -p mediamaster-agent
```

> The `mf`/`mediacodec` features are **disabled by default** and the crates are
> published without them, so native scaffold code does not reach crates.io.
> On target devices, enable `mf`/`mediacodec` for the native path.

## 6. Tag & verify

```bash
git tag -a v0.3.0 -m "mediamaster-rs 0.3.0 expansion"
git push origin v0.3.0
```

Verify each crate page on crates.io shows the new version, and the README
badges still render (they fetch live from crates.io).

## 7. Decision points (resolve before executing)

1. **Publish all four new crates?** Yes — `probe`, `codec`, `agent` all publish
   at 0.3.0. The `codec`'s `mf`/`mediacodec` features stay **disabled in the
   published crate** (device-validation-pending on Windows/Android).
2. **The baseline crates (`audio`, `rf`, `sensors`, `bridge`) stay at 0.2.0/0.1.0** —
   they are not re-published in this run.
3. **Token owner** — `cargo login` requires a crates.io API token. Confirm the
   token owner before starting — publishing is **irreversible** (crates.io does
   not allow un-publishing a version).
4. **`publish = false` removed?** `probe` and `codec` `publish = false` lines are
   removed for this release; `integration-tests` kept `publish = false` (test
   harness, not publishable).

## 8. After the release

- Move `CHANGELOG.md`'s `[Unreleased]` heading into a `## [0.3.0] — <date>` section.
- Confirm the README `Crates.io` table/badges reflect the new versions.
- `masteraudio-rs.md` §6 Phase 1.5 → ✅ done.
