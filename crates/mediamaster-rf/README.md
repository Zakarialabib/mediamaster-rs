# mediamaster-rf

SDR baseband DSP pipeline: IQ acquisition, FFT detection, and protocol parsing.

## What it provides

- `IqSource` — the acquisition trait (RTL-SDR / SoapySDR / synthetic).
- `Pipeline` — the 3-stage async pipeline: acquire → FFT/detect → parse.
- `MagnitudeDetector` — `rustfft`-backed FFT + peak detector, implements
  `SignalDetector` from `mediamaster-core` so the event bus can be fed.
- `ProtocolParser` — `nom` combinators for sync word → header → payload → CRC.

## Data flow

```text
SDR hardware
   │
   ▼
IqSource → acquire() ──▶ Stage 2 (FFT + SignalDetector)
   │                               │
   ▼                               ▼
Stage 1 (batch)           RadioBurstDetected ──▶ EventBus
   │
   ▼
IqBatch ───▶ Stage 3 (ProtocolParser  ──▶ emit event
```

## Examples

- `decode_433mhz_weather.rs`
- `custom_protocol.rs`
- `rtl_sdr_capture.rs`
