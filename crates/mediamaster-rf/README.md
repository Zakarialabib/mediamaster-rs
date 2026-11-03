# mediamaster-rf

SDR baseband DSP pipeline: IQ acquisition, FFT detection, OOK demodulation, and
protocol parsing.

## What it provides

- `IqSource` — the acquisition trait (RTL-SDR / SoapySDR / synthetic).
- `MagnitudeDetector` — `rustfft`-backed FFT + peak detector, implements
  `SignalDetector` from `mediamaster-core`.
- `OokDemodulator` — OOK/ASK envelope demodulator (315/433 MHz ISM standard):
  recovers a byte stream from I/Q by averaging the envelope over
  `samples_per_symbol` and slicing against a threshold.
- `ProtocolParser` — `nom` combinators for sync word → header → payload.
- `Pipeline` — three-stage async pipeline (acquisition → DSP → demod/parse) driven by
  the caller's tokio runtime; publishes `RadioBurstDetected` to the `EventBus`.

## Data flow

```text
SDR hardware
   │
   ▼
IqSource → acquire() ──▶ AcquisitionStage::run()   (stage 1)
   │                               │
   ▼                               ▼
IqBatch (loop)         MagnitudeDetector::detect() → Signal   (stage 2)
   │                               │
   ▼                               ▼
   └──────▶ Pipeline::run() (stage 3: demod + parse) ──┘
```

## Feature flags

- `rtl-sdr` — RTL-SDR backend (via the `rtlsdr` crate). **NOTE**: This feature is
  currently **abandoned** — the upstream `rtlsdr 0.1.4` crate's API does not match the
  expected signature, and no compatible async version is available. The `Pipeline` and
  demodulator are feature-complete but do not compile with `rtl-sdr`.
- `std` (default) — includes `std` support.
- `alloc` — `no_std` + `alloc` builds.

## Publishing

Published to crates.io: [`mediamaster-rf`](https://crates.io/crates/mediamaster-rf) v0.2.0.
