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
- `Pipeline` — three-stage async pipeline (acquisition → DSP → demod/parse)
  driven by the caller's tokio runtime; publishes `RadioBurstDetected` to the
  `EventBus`. `AcquisitionStage` is its stage-1 facade.

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
OokDemodulator::demodulate() → ProtocolParser::parse_sync()   (stage 3)
   │                               │
   ▼                               ▼
byte stream             emit RadioBurstDetected to EventBus
```

## Usage

```rust
use mediamaster_rf::{MagnitudeDetector, OokDemodulator, ProtocolParser};
use mediamaster_core::traits::SignalDetector;

// Peak detection: only magnitudes above the threshold count as signals.
let detector = MagnitudeDetector::new(256, 0.5);
let magnitudes = vec![0.1, 0.8, 0.3, 0.9];
let mut signals = Vec::new();
let peaks = detector.detect(&magnitudes, &mut signals);
// peaks == 2, signals[0].id == "1", signals[1].id == "3"

// OOK demodulation: recover bytes from I/Q samples.
let demod = OokDemodulator::new(0.5, 4);
// demod.demodulate(&samples) -> Vec<u8>
```
