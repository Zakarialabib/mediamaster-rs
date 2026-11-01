# Release plan — mediamaster-rs 0.2.0

> **Status:** plan (not yet executed) · **Audience:** core team
> **Companion:** [`CHANGELOG.md`](../CHANGELOG.md) (`[Unreleased]` section) ·
> [`masteraudio-rs.md`](masteraudio-rs.md) §6 (phase table)

This is the copy-paste plan for the next crates.io release. It is a **plan** —
nothing here has been run yet. Read §1–§3 first (versions + dependency re-pins),
then execute §4–§7 in order.

## 1. Current state (verified 2026-11-01)

| Crate | Version on crates.io | Published | `publish` in Cargo.toml |
|-------|---------------------|-----------|-------------------------|
| `mediamaster-core` | 0.1.1 | 2026-10-27 | default (true) |
| `mediamaster-audio` | 0.1.1 | 2026-10-27 | default (true) |
| `mediamaster-rf` | 0.1.0 | 2026-10-27 | default (true) |
| `mediamaster-sensors` | 0.1.1 | 2026-10-27 | default (true) |
| `mediamaster-bridge` | — | **not published** | `publish = false` |
| `mediamaster-integration-tests` | — | not published | `publish = false` |

Everything merged to `main` after 2026-10-27 (bridge, `EventHandlers`, OOK
demod, 3-stage pipeline, zones, ducking profiles) is **unreleased** — it builds
and is tested (70 tests) but is not on crates.io.

## 2. Target versions

A **coordinated 0.2.0** across the four published crates, plus a first publish
of `mediamaster-bridge` at 0.1.0. One version number keeps the workspace
trivially aligned (one `mediamaster-core` in every consumer's tree — no
duplicate-type split).

| Crate | From → To | Why |
|-------|-----------|-----|
| `mediamaster-core` | 0.1.1 → **0.2.0** | breaking: new `HardwareEvent::ZoneChanged` variant |
| `mediamaster-audio` | 0.1.1 → **0.2.0** | additive (zones + `DuckingProfile`); aligned to core 0.2 |
| `mediamaster-rf` | 0.1.0 → **0.2.0** | breaking: `Pipeline::new` gained `demod`; `samples_to_bytes` removed |
| `mediamaster-sensors` | 0.1.1 → **0.2.0** | no code change; re-pins core `^0.2.0` (avoids two core copies) |
| `mediamaster-bridge` | — → **0.1.0** | first publish (app facade) |

> **Semver-accurate alternative** (if you prefer strict 0.x semantics over a
> locked workspace): core 0.2.0, rf 0.2.0, audio 0.1.2, sensors 0.1.2,
> bridge 0.1.0. Same steps, different numbers — but mixed versions mean more
> to document and verify. This plan uses the **aligned 0.2.0** scheme.

## 3. Dependency re-pins (required before publishing)

Every crate pins its internal deps with a `version` alongside `path`. A core
bump to 0.2.0 means each dependent's `mediamaster-core = { version = "..." }`
must move to `"0.2.0"` — otherwise cargo resolves a second, semver-incompatible
`mediamaster-core` 0.1.x into the tree and `RoomState`/`EventBus` types no
longer unify across crates.

| File | Line (dep) | Change `version` to |
|------|-----------|---------------------|
| `crates/mediamaster-audio/Cargo.toml` | `mediamaster-core` | `"0.2.0"` |
| `crates/mediamaster-rf/Cargo.toml` | `mediamaster-core` | `"0.2.0"` |
| `crates/mediamaster-sensors/Cargo.toml` | `mediamaster-core` | `"0.2.0"` |
| `crates/mediamaster-bridge/Cargo.toml` | `mediamaster-core` | `"0.2.0"` |
| `crates/mediamaster-bridge/Cargo.toml` | `mediamaster-audio` | `"0.2.0"` |
| `crates/mediamaster-bridge/Cargo.toml` | `mediamaster-rf` | `"0.2.0"` |
| `crates/mediamaster-bridge/Cargo.toml` | `mediamaster-sensors` | `"0.2.0"` |
| `crates/mediamaster-integration-tests/Cargo.toml` | all four | `"0.2.0"` |

Also bump each crate's own `version` field per §2 (core/audio/rf/sensors →
`0.2.0`; bridge stays `0.1.0`).

## 4. Pre-flight (must be green)

```bash
cargo fmt --check
cargo clippy --workspace --all-features --all-targets -- -D warnings
cargo test --workspace --all-features        # 70 tests, 0 failures
```

## 5. Publish order (dependency-first)

crates.io must index a dependency before a dependent can resolve it, so publish
in this order and **wait for indexing** between steps (a minute or two; check
the crate page loads):

1. **`mediamaster-core`** (no internal deps)
2. **`mediamaster-audio`**, **`mediamaster-rf`**, **`mediamaster-sensors`**
   (each only needs `core` — can go in any order, ideally after core is indexed)
3. **`mediamaster-bridge`** (needs all four; see §7 decision)

For each crate: remove `publish = false` if present (bridge only), then dry-run,
then publish:

```bash
# from the crate directory (or -p <crate> from the workspace root)
cargo publish --dry-run -p mediamaster-core    # verify packaging first
cargo publish -p mediamaster-core              # real publish
# …wait for crates.io to index core, then repeat for audio/rf/sensors, then bridge
```

> `cargo publish` for a path dependency requires the `version` field to match a
> version that exists on crates.io — that is exactly why §3's re-pins must land
> **before** publishing the dependents.

## 6. Tag & verify

```bash
git tag -a v0.2.0 -m "mediamaster-rs 0.2.0"
git push origin v0.2.0
```

Verify each crate page on crates.io shows the new version, and the README
badges still render (they fetch live from crates.io).

## 7. Decision points (resolve before executing)

1. **Publish `mediamaster-bridge`?** It is `publish = false` today and apps use
   it via `path`. Recommended: **publish it at 0.1.0** — it is a complete,
   tested facade and lets external apps depend on it from crates.io. To publish,
   delete the `publish = false` line in its `Cargo.toml` first. If you would
   rather keep it in-repo only, skip step 3 in §5 and leave the flag set.
2. **Aligned 0.2.0 vs semver-accurate** — this plan uses aligned 0.2.0 (§2).
   Switch to the alternative numbers only if you want strict 0.x semantics.
3. **Who runs the publish?** `cargo publish` needs a crates.io API token
   (`cargo login`). Confirm the token owner before starting — publishing is
   **irreversible** (crates.io does not allow un-publishing a version).

## 8. After the release

- Move `CHANGELOG.md`'s `[Unreleased]` heading into a `## [0.2.0] — <date>` section.
- Confirm the README `Crates.io` table/badges reflect the new versions.
- `masteraudio-rs.md` §6 Phase 1.5 → ✅ done.
