# mediamaster-rf

SDR baseband DSP pipeline: IQ acquisition, FFT detection, and protocol parsing.

## What it provides

- `IqSource` — the acquisition trait (RTL-SDR / SoapySDR / synthetic).
- `AcquisitionStage` — the 3-stage async pipeline: acquire → FFT/detect → parse.
- `MagnitudeDetector` — `rustfft`-backed FFT + peak detector, implements
  `SignalDetector` from `mediamaster-core` so the event bus can be fed.
- `ProtocolParser` — `nom` combinators for sync word → header → payload → CRC.

## Data flow

```text
SDR hardware
   │
   ▼
IqSource → acquire() ──▶ AcquisitionStage::run()
   │                               │
   ▼                               ▼
IqBatch (loop)         MagnitudeDetector::detect() → Signal
   │                               │
   ▼                               ▼
Stage 3 (ProtocolParser ──▶  emit RadioBurstDetected
         parse_sync)        to EventBus
```

## Usage

```rust
use mediamaster_rf::{MagnitudeDetector, ProtocolParser};
use mediamaster_core::traits::SignalDetector;

let detector = MagnitudeDetector::new(256, -30.0);
let magnitudes = vec![0.1, 0.8, 0.3, 0.9];
let mut signals = Vec::new();
let peaks = detector.detect(&magnitudes, &mut signals);
// peaks == 2, signals[0].id == "1", signals[1].id == "3"
```
