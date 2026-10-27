**Do NOT integrate audio/SDR/sensors anything , we are brainstorming to comeup for the best for each case , tv-rs cctv-rs or signagemater windows app, or signagemaster-companion android:

- `tv-rs` = TV tuner/broadcast domain (video + RF tuning)
- `cctv-rs` = Security camera domain (video capture + recording)
- Audio mixing, SDR baseband DSP, and sensor fusion are **orthogonal concerns**
- Integrating them would violate single-responsibility, bloat dependencies, and limit reusability

We speculate that Creating a new crate , extractomg signage logic domain-specific crates that form a **hardware mediamastertion ecosystem** alongside `tv-rs` and `cctv-rs`.

---

## 📦 Crate Naming Strategy

Use a **shared prefix** for ecosystem discoverability while keeping names functional and searchable:

### Recommended: `mediamaster-*` Suite 

| Crate Name | Purpose | Dependencies |
|------------|---------|--------------|
| `mediamaster-core` | Shared event bus, traits, types | `tokio`, `serde`, `thiserror` |
| `mediamaster-audio` | Lock-free PCM mixing, ducking, routing | `mediamaster-core`, `cpal`, `symphonia`, `ringbuf` |
| `mediamaster-rf` | SDR IQ pipeline, FFT detection, protocol parsing | `mediamaster-core`, `rustfft`, `nom`, `num-complex` |
| `mediamaster-sensors` | Async sensor polling, debouncing, context fusion | `mediamaster-core`, `embedded-hal-async`, `tokio` |

### Alternative: Independent Functional Names
(Highly searchable, no prefix coupling)

| Crate Name | Purpose |
|------------|---------|
| `hw-events` | Shared event bus and traits |
| `pcm-mixer` | Audio mixing and ducking |
| `sdr-pipeline` | SDR DSP and protocol parsing |
| `sensor-hub` | Sensor abstraction and fusion |

**My recommendation**: Use `mediamaster-*` for ecosystem cohesion. It signals these crates work together while remaining distinct.

---

## 🗂️ Project Structure

```text
mediamaster/
├── Cargo.toml                    # Workspace manifest
├── README.md                     # Ecosystem overview
├── LICENSE-MIT
├── LICENSE-APACHE
│
├── crates/
│   │
│   ├── mediamaster-core/           # THE FOUNDATION
│   │   ├── Cargo.toml
│   │   ├── README.md
│   │   ├── src/
│   │   │   ├── lib.rs            # Public API exports
│   │   │   ├── events.rs         # HardwareEvent enum
│   │   │   ├── traits.rs         # AudioSource, AsyncSensor, SignalDetector
│   │   │   ├── types.rs          # Shared types (AudioTarget, RoomState, etc.)
│   │   │   └── error.rs          # mediamasterError
│   │   ├── examples/
│   │   │   └── event_bus_demo.rs # Shows how to publish/subscribe
│   │   └── tests/
│   │       └── event_tests.rs
│   │
│   ├── mediamaster-audio/          # AUDIO DOMAIN
│   │   ├── Cargo.toml
│   │   ├── README.md
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── mixer.rs          # Lock-free mixing logic
│   │   │   ├── ducking.rs        # Volume interpolation
│   │   │   ├── sink.rs           # cpal integration
│   │   │   └── source.rs         # AudioSource trait impl helpers
│   │   ├── examples/
│   │   │   ├── mix_two_sine_waves.rs
│   │   │   ├── tv_rs_adapter.rs  # Shows how to adapt tv-rs
│   │   │   └── duck_on_sensor_event.rs
│   │   ├── benches/
│   │   │   └── mixer_throughput.rs
│   │   └── tests/
│   │       └── mixer_tests.rs
│   │
│   ├── mediamaster-rf/             # RADIO/RF DOMAIN
│   │   ├── Cargo.toml
│   │   ├── README.md
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── pipeline.rs       # 3-stage tokio pipeline
│   │   │   ├── detector.rs       # FFT magnitude analysis
│   │   │   ├── parser.rs         # nom combinator helpers
│   │   │   └── iq_source.rs      # IqSource trait
│   │   ├── examples/
│   │   │   ├── decode_433mhz_weather.rs
│   │   │   ├── custom_protocol.rs
│   │   │   └── rtl_sdr_capture.rs
│   │   └── tests/
│   │       └── pipeline_tests.rs
│   │
│   └── mediamaster-sensors/        # SENSOR DOMAIN
│       ├── Cargo.toml
│       ├── README.md
│       ├── src/
│       │   ├── lib.rs
│       │   ├── debouncer.rs      # Time-based debouncing
│       │   ├── aggregator.rs     # Context state fusion
│       │   ├── sensor.rs         # AsyncSensor trait wrapper
│       │   └── fusion.rs         # FusionEngine
│       ├── examples/
│       │   ├── i2c_light_sensor.rs
│       │   ├── serial_pir_motion.rs
│       │   └── multi_sensor_fusion.rs
│       └── tests/
│           └── debouncer_tests.rs
│
└── docs/
    ├── architecture.md           # How the crates work together
    ├── integration-guide.md      # How to adapt tv-rs/cctv-rs
    └── api-reference.md          # Generated rustdoc links
```

---

## 🧠 Logic & Context for Agent (No Code)

### 1. **`mediamaster-core` — The Shared Vocabulary**

**Purpose**: Define the event types and traits that allow disparate hardware domains to communicate without direct dependencies.

**Key Concepts**:
- **Event Bus**: A `tokio::sync::broadcast` channel that all crates publish to and subscribe from
- **Event Types**: `AudioDuckRequest`, `SensorContextChange`, `RadioBurstDetected`, `CctvMotionDetected`
- **Traits**: `AudioSource` (yields PCM frames), `AsyncSensor` (reads hardware), `SignalDetector` (identifies RF patterns)
- **Zero Hardware Dependencies**: Only depends on `tokio`, `serde`, `thiserror`

**Integration Pattern**:
- `tv-rs` and `cctv-rs` do NOT depend on `mediamaster-core`
- Your Tauri app (or a bridge crate) adapts them by implementing `mediamaster-core` traits
- Example: Create a `TvRsAudioAdapter` that implements `AudioSource` by extracting PCM from `tv-rs`

---

### 2. **`mediamaster-audio` — Lock-Free PCM Mixing**

**Purpose**: Mix multiple audio sources (TV audio, CCTV mic, system alerts) with real-time ducking, without blocking the async runtime.

**Key Concepts**:
- **Lock-Free Queues**: Use `ringbuf::HeapRb` (SPSC) to pass PCM frames between async tasks and the `cpal` audio thread
- **Software Mixing**: Sum `f32` PCM samples from multiple sources, apply per-source gain
- **Ducking**: When a high-priority event occurs (e.g., sensor alert), smoothly interpolate background audio volume down
- **`cpal` Sink**: The final output thread reads from a ring buffer and writes to OS audio hardware

**Data Flow**:
1. Multiple `AudioSource` implementations (adapters for `tv-rs`, `cctv-rs`, system alerts) push PCM frames into separate ring buffers
2. A mixer task reads from all buffers, applies ducking multipliers, sums samples, writes to output ring buffer
3. `cpal` callback reads from output ring buffer (zero allocations, zero blocking)

**Integration with `tv-rs`**:
- Your Tauri app creates a `TvRsAudioAdapter` that implements `AudioSource`
- The adapter calls `tv-rs` decode methods and yields PCM frames
- Pass the adapter to `mediamaster-audio::Mixer::add_source()`

---

### 3. **`mediamaster-rf` — SDR DSP Pipeline**

**Purpose**: Continuously monitor RF spectrum, detect signal patterns, parse binary protocols, and emit events.

**Key Concepts**:
- **3-Stage Tokio Pipeline**:
  1. **IQ Acquisition**: Blocking task reads raw I/Q samples from SDR hardware (via `rtlsdr` or `soapysdr-rs`), batches them, sends to stage 2
  2. **DSP & Detection**: Async task applies FFT (via `rustfft`), scans magnitude array for peaks, extracts time-domain bytes of detected bursts
  3. **Protocol Parsing**: Async task uses `nom` combinators to parse binary protocol (sync word → header → payload → CRC), emits `RadioBurstDetected` event
- **Trait-Based Extensibility**: User implements `SignalDetector` (defines what a "signal" looks like) and `ProtocolParser` (defines how to parse bytes)

**Data Flow**:
1. SDR hardware → `IqSource` trait → Stage 1 (batching) → Stage 2 (FFT + detection) → Stage 3 (parsing) → `mediamaster-core` event bus

**Integration with Signage**:
- When `RadioBurstDetected` event fires, your Tauri app can trigger `tv-rs` to display an overlay or `cctv-rs` to start recording

---

### 4. **`mediamaster-sensors` — Async Sensor Fusion**

**Purpose**: Abstract noisy hardware sensors into reliable, high-level "context states" that drive signage behavior.

**Key Concepts**:
- **`embedded-hal-async` Traits**: Standard async interfaces for I2C, SPI, UART sensors
- **Debouncing**: `DebouncedSensor` wrapper ignores transient noise by requiring stable readings over a time window
- **Context Fusion**: `FusionEngine` aggregates multiple sensor readings into a `ContextState` (e.g., `RoomOccupied`, `LowLight`, `MotionDetected`)
- **Async Polling**: Use `tokio::time::interval` to poll sensors without blocking

**Data Flow**:
1. Hardware sensors implement `AsyncSensor` trait
2. Wrap each in `DebouncedSensor` for noise filtering
3. `FusionEngine` polls all debounced sensors at fixed interval
4. When `ContextState` changes, emit `SensorContextChange` event to bus

**Integration with Signage**:
- When `SensorContextChange { state: RoomOccupied }` fires, your Tauri app can:
  - Send command to `tv-rs` to power on display
  - Send command to `cctv-rs` to start recording
  - Send `AudioDuckRequest` to `mediamaster-audio` to lower background music
