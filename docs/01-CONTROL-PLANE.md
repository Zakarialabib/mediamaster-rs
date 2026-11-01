# Control Plane — tv-rs & desktop↔mobile sync

> Inspection + integration seam (2026-10-27). Maps `mediamaster-rs`'s
> capability vocabulary onto the `tv-rs` fleet model and the Simple-Signage
> desktop↔mobile sync/control transport. Non-normative reference for Phase 2.

## 1. `tv-rs` device model (the pattern to mirror)

`tv-rs-core` = **"vocabulary, not protocol"** (no tokio, no I/O):
- `DeviceHandle` — erased, capability-downcastable handle (`as_power()`,
  `as_volume()`, `as_key()`, `as_input_select()`, …). `None` ⇒
  `TvError::Unsupported`, knowable *before* the attempt.
- `Capability` / `CapabilitySet` — what a device *can* do.
- `state::ObservedState` — confidence-tagged (`Confidence::Unknown` ≠ off).
- `error::TvError` — routing taxonomy (re-pair / reconnect / back-off / stop).
- `agent` crate — pure reconciler: `DesiredState` + `ObservedState` +
  `CapabilitySet` → `Decision` (`decide(..)` is pure); `healer` (retry ladder);
  `audit` (JSONOL events). Refuses to decide content/schedule; quarantine,
  don't toggle.

Facade (`tv-rs` lib): `discover()` (mDNS/SSDP/UPnP) → `tv.to_client()` →
`client.power_off()`; per-vendor protocols via backend crates + typed accessors.

**Mirror for `mediamaster-rs`:** `core` = capability traits (`AudioSource`,
`AsyncSensor`, `SignalDetector`) + the `EventBus`/`HardwareEvent` vocabulary;
backends live in `core`, the **apps compose a reconciler** over `EventBus`.
No I/O in the trait crates.

## 2. Simple-Signage desktop↔mobile control + sync

Transport stack lives in `crates/player/src/sync/` (imported from
`src-tauri/src/lib.rs` as `::player::sync`):

- **Discovery** — `discovery.rs` (`MdnsBrowser`) + `lan_display_discovery`
  (mDNS/SSDP/UPnP): phones browse the LAN for players & displays.
- **Pairing** — `pairing.rs`: ed25519-signed bearer tokens via
  `POST /api/v1/pair` (anchors B10 `fleet_status`, B4 `boot_first_frame`).
- **Status sync (desktop → mobile)** — `control_ws.rs` (`WS /api/v1/control`)
  + `ws_client.rs`: server pushes `WsPlayerStatus` snapshots
  (`show_id`, `slide_index`, `status`, `uptime`, `kiosk_mode`, `device_name`,
  …); companion subscribes `{ "type":"subscribe", "token":"<token>" }`.
  Rate-limit: 64 KB/frame, 50 cmds/sec/socket, 5-min nonce TTL.
- **Control (mobile → desktop)** — HTTP REST on the sync server carrying the
  shared `playerBridge` verb set (`playerBridge.ts`):
  `play, pause, next, prev, override, volume` (+ `seek`/`stop`). WS is
  **status-only**; commands go REST. Fan-out is full-mesh and may target a
  specific display by device id.
- **App integration** — each validated command is re-emitted as a `player:*`
  Tauri event so `TvModeRoot` + the on-screen player react identically to a
  local UI input.

## 3. `mediamaster-rs` architecture (current, authoritative)

Built & (aspirationally) published as crates.io `0.1.x`; README/integration
pattern is the source of truth:

- `mediamaster-core` — `AudioSource` trait (`read_pcm(&self,&mut [f32])->usize`, `label`),
  `HardwareEvent` (incl. `AudioDuckRequest{source,ducking,until:u64}`), `EventBus`
  (`subscribe()→broadcast::Receiver`, `publish()→bool`). tokio lives in **core** only.
- `mediamaster-audio` — `Mixer` is **pure** (no tokio): `Mixer::new()`,
  `add_source`, `set_ducking/ducking`, `mix(&mut self)->Vec<f32>`; under
  `audio-calc` an SPSC `HeapRb` + `feed(&self)`/`LockRead` drain for the cpal
  callback (`#[cfg] unsafe impl Sync`).
- `mediamaster-rf` — `SignalDetector` + `RadioBurst`.
- `mediamaster-sensors` — `AsyncSensor` + `FusionEngine` (confidence state).

**Compositional rule (apps wire crates directly — no bridge crate):**
each app implements `AudioSource`/`AsyncSensor` and owns its `Mixer`,
draining with `mix()`/`mix_step()` every frame.

## 4. Phase-2 integration seam (the innovation to implement)

The `player:*`/playerBridge transport drives the mixer through `EventBus`.
The host-side `PluginAudio` owns a `Mixer` + a `broadcast::Receiver`, and on
each frame drains events then mixes:

```rust
// app PluginAudio — the desktop seam (mirrors tv-rs `agent::decide`)
let mut rx = event_bus.subscribe();
while let Ok(ev) = rx.try_recv() {
    if let HardwareEvent::AudioDuckRequest { ducking, until } = &ev {
        let now = monotonic_now_ms(); // app-owned clock (u64 ms, monotonic)
        mixer.set_ducking_profile(DuckingProfile {
            curve: DuckingCurve::Exponential,
            duration_ms: until.saturating_sub(now),
            target: *ducking,
        });
    }
}
let frames = mixer.mix();
// → write `frames` to the media-engine output (crates/media/audio.rs)
```

`playerBridge` verb → transport → `EventBus` mapping:

| playerBridge verb (REST) | Re-emitted Tauri event | mediamaster path |
|---|---|---|
| `volume`/`mute` | `player:set_volume` | `AudioSource` → `Mixer` (`set_ducking`) |
| `override` / duck trigger | `player:audio_duck` | `AudioDuckRequest` → app builds `DuckingProfile` (`dur=until-now`, `target=ducking`) → `Mixer::set_ducking_profile` |
| sensor/scene | `player:scene_*` | `AsyncSensor` / `FusionEngine` (`SensorContextChange`→`LowLight` dim) |
| `play/pause/next` | `player:navigate` | — (playlist/media-engine; outside mediamaster scope) |

**Time-aware ducking (the gap `until: u64` exposes):** `HardwareEvent::AudioDuckRequest`
carries an expiry (`until`). The app schedules `Mixer::set_ducking(ducking)` on
arrival and restores the prior gain when `now >= until` (monotonic timestamp;
clamp `ducking` to `[0,1]`, treat `until==0` as immediate/once). This is the
`0.2.0` increment: keep `Mixer` pure-tokio-free; the schedule/restore lives
in the app's `PluginAudio` (single owner of `Mixer` + clock).

## 5. Status / next

- **Aligned**: removed the stray `mediamaster-simple-signage` bridge crate
  (contradicted the canonical "apps wire crates directly" rule). Workspace
  green: `cargo fmt --check` clean, `cargo test --workspace` → 42 pass / 0 fail
  (core/audio/rf/sensors).
- **Not committed**: `mediamaster-rs` is not yet a git repo (README's crates.io
  / CI badges are aspirational); `Simple-Signage` is clean at `feat/player-snapshot`.
- **Phase 2**: the host-side `PluginAudio` wiring above (Simple-Signage domain).
- **`0.2.0`**: add `Mixer::apply_event(&HardwareEvent)` (pure: matches
  `AudioDuckRequest` → `set_ducking`; no tokio) so the app's EventBus drain is a
  one-liner, plus document the `until`-restoration schedule.
