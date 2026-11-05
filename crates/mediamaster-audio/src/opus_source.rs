//! Opus decoding audio source for push-to-talk live paging.
//!
//! A mobile companion encodes microphone audio to Opus packets (typically 20ms
//! frames at 48kHz). This module receives packets into a packet queue, drains
//! them into PCM samples using libopus (or PLC when packets are lost), and pushes
//! them into a [`PagingJitterBuffer`], implementing [`AudioSource`] so it can
//! directly feed the audio [`Mixer`].
//!
//! # Licensing
//! Native Opus decoding links `libopus` via the `opus` crate. Libopus is licensed
//! under the BSD 3-Clause license.
//!
//! When compiling without native FFI (e.g. target platforms lacking FFI or using
//! `opus-hybrid`), a pure-Rust fallback stub is provided which returns
//! [`OpusError::UnsupportedCodec`].

use mediamaster_core::traits::AudioSource;
use std::collections::VecDeque;
use std::sync::Mutex;

use crate::jitter::PagingJitterBuffer;

/// Standard Opus voice frame duration in samples at 48kHz (20ms = 960 samples).
pub const OPUS_FRAME_SAMPLES_20MS: usize = 960;
pub const OPUS_SAMPLE_RATE: u32 = 48_000;

#[derive(Debug, thiserror::Error)]
pub enum OpusError {
    #[error("unsupported codec: native Opus FFI is not compiled in")]
    UnsupportedCodec,
    #[error("opus decode error: {0}")]
    DecodeError(String),
    #[error("opus encode error: {0}")]
    EncodeError(String),
}

/// A packet queued for decoding, with sequence numbering to detect loss / reordering.
#[derive(Debug, Clone)]
pub struct PagingPacket {
    pub sequence: u64,
    pub payload: Vec<u8>,
}

/// Inner state of `OpusPagingSource`.
struct OpusDecoderState {
    jitter: PagingJitterBuffer,
    packet_queue: VecDeque<PagingPacket>,
    #[cfg(feature = "opus")]
    expected_sequence: Option<u64>,
    #[cfg(feature = "opus")]
    decoder: opus::Decoder,
}

/// An `AudioSource` that receives Opus frames from push-to-talk companion clients,
/// decodes them into PCM via Opus with packet loss concealment (PLC), and yields
/// smoothed PCM via its internal `PagingJitterBuffer`.
pub struct OpusPagingSource {
    label: String,
    state: Mutex<OpusDecoderState>,
}

impl OpusPagingSource {
    /// Create a new `OpusPagingSource`.
    ///
    /// Under the `opus` feature, creates an Opus decoder for 48kHz mono.
    /// Under `opus-hybrid` without `opus`, returns `Err(OpusError::UnsupportedCodec)`.
    pub fn new(label: impl Into<String>) -> Result<Self, OpusError> {
        Self::with_jitter_config(label, OPUS_SAMPLE_RATE, 50)
    }

    /// Create with specific jitter buffer sample rate and prebuffer threshold (in ms).
    pub fn with_jitter_config(
        label: impl Into<String>,
        sample_rate: u32,
        prebuffer_ms: u32,
    ) -> Result<Self, OpusError> {
        #[cfg(feature = "opus")]
        {
            let decoder = opus::Decoder::new(sample_rate, opus::Channels::Mono)
                .map_err(|e| OpusError::DecodeError(e.to_string()))?;
            Ok(Self {
                label: label.into(),
                state: Mutex::new(OpusDecoderState {
                    jitter: PagingJitterBuffer::new(sample_rate, prebuffer_ms),
                    packet_queue: VecDeque::new(),
                    expected_sequence: None,
                    decoder,
                }),
            })
        }
        #[cfg(not(feature = "opus"))]
        {
            let _ = (sample_rate, prebuffer_ms);
            let _ = label.into();
            Err(OpusError::UnsupportedCodec)
        }
    }

    /// Push an Opus packet from the network into the decoder's incoming queue.
    /// Packets will be decoded in order during `read_pcm`.
    pub fn push_packet(&self, sequence: u64, payload: Vec<u8>) {
        if let Ok(mut state) = self.state.lock() {
            let pkt = PagingPacket { sequence, payload };
            // Insert sorted by sequence number to handle network reordering
            let pos = state
                .packet_queue
                .iter()
                .position(|p| p.sequence > sequence)
                .unwrap_or(state.packet_queue.len());
            state.packet_queue.insert(pos, pkt);
        }
    }

    /// Number of packets waiting in the incoming queue.
    pub fn queued_packet_count(&self) -> usize {
        self.state.lock().map(|s| s.packet_queue.len()).unwrap_or(0)
    }

    /// Whether the underlying jitter buffer has completed its prebuffer warmup.
    pub fn is_primed(&self) -> bool {
        self.state
            .lock()
            .map(|s| s.jitter.is_primed())
            .unwrap_or(false)
    }

    /// Decode available packets from the queue and push PCM into the jitter buffer.
    #[cfg(feature = "opus")]
    fn pump_decoding(state: &mut OpusDecoderState) {
        let mut pcm_out = [0.0f32; OPUS_FRAME_SAMPLES_20MS];

        while let Some(packet) = state.packet_queue.pop_front() {
            if let Some(expected) = state.expected_sequence {
                if packet.sequence > expected {
                    // Packets were lost! Run PLC (Packet Loss Concealment)
                    let lost_count = (packet.sequence - expected).min(10); // cap PLC bursts to 10 frames (200ms)
                    for _ in 0..lost_count {
                        // In Opus, decode_float with empty/None payload triggers PLC
                        if let Ok(samples) = state.decoder.decode_float(&[], &mut pcm_out, true) {
                            state.jitter.push(&pcm_out[..samples]);
                        }
                    }
                } else if packet.sequence < expected {
                    // Stale / duplicate packet arrived late after gap, ignore
                    continue;
                }
            }

            state.expected_sequence = Some(packet.sequence + 1);

            // Normal packet decode
            match state
                .decoder
                .decode_float(&packet.payload, &mut pcm_out, false)
            {
                Ok(samples) => {
                    state.jitter.push(&pcm_out[..samples]);
                }
                Err(_) => {
                    // On corrupted packet, invoke PLC
                    if let Ok(samples) = state.decoder.decode_float(&[], &mut pcm_out, true) {
                        state.jitter.push(&pcm_out[..samples]);
                    }
                }
            }
        }
    }
}

impl AudioSource for OpusPagingSource {
    fn read_pcm(&self, buffer: &mut [f32]) -> usize {
        #[cfg(feature = "opus")]
        {
            let mut state = match self.state.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };

            // Process any pending packets into jitter buffer
            Self::pump_decoding(&mut state);

            let mut count = 0;
            for sample in buffer.iter_mut() {
                *sample = state.jitter.pop_output();
                count += 1;
            }
            count
        }
        #[cfg(not(feature = "opus"))]
        {
            for sample in buffer.iter_mut() {
                *sample = 0.0;
            }
            buffer.len()
        }
    }

    fn label(&self) -> &str {
        &self.label
    }
}
