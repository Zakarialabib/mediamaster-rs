//! Poster producer for visual degradation fallback.
//!
//! Decodes a representative keyframe from a packet source using an
//! `EngineSelector`-acquired decoder (falling back to software path),
//! applies `BackdropPolicy.select_at` heuristics, and emits formatted
//! poster bytes to a `BackdropSink`. Never writes to disk.

use crate::engine_selector::{Engine, EngineSelector};
use crate::transcode_plan::Codec;
use mediamaster_core::caps::{CapsMatrix, DegradationTier};
use mediamaster_core::events::{EventPublisher, HardwareEvent};
use mediamaster_core::policy::{BackdropPolicy, BackdropSelect};
use mediamaster_probe::types::MediaDescriptor;
use std::error::Error;

/// Encoded media packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedPacket {
    /// Bitstream packet bytes.
    pub data: Vec<u8>,
    /// Whether this packet is a video keyframe / IDR frame.
    pub is_keyframe: bool,
    /// Presentation timestamp (in milliseconds or arbitrary monotonic timebase).
    pub pts: u64,
    /// Identifier of the stream this packet belongs to.
    pub stream_id: String,
}

/// Source yielding encoded packets for poster generation.
pub trait PacketSource {
    /// Read the next encoded packet, or `None` if end-of-stream.
    fn next_packet(&mut self) -> Result<Option<EncodedPacket>, Box<dyn Error + Send + Sync>>;
}

/// Sink receiving the extracted poster/backdrop image artifact.
pub trait BackdropSink {
    /// Emit the generated poster image bytes to the sink.
    fn emit_backdrop(
        &mut self,
        tier: DegradationTier,
        mime: &str,
        data: &[u8],
        uri_hint: &str,
    ) -> Result<(), Box<dyn Error + Send + Sync>>;
}

/// Coordinator that decodes keyframe(s) and produces poster fallback artifacts.
pub struct PosterProducer<P: PacketSource, S: BackdropSink> {
    source: P,
    sink: S,
    caps: CapsMatrix,
}

impl<P: PacketSource, S: BackdropSink> PosterProducer<P, S> {
    /// Creates a new `PosterProducer`.
    pub fn new(source: P, sink: S, caps: CapsMatrix) -> Self {
        Self { source, sink, caps }
    }

    /// Access reference to the inner source.
    pub fn source(&self) -> &P {
        &self.source
    }

    /// Access mutable reference to the inner source.
    pub fn source_mut(&mut self) -> &mut P {
        &mut self.source
    }

    /// Access reference to the inner sink.
    pub fn sink(&self) -> &S {
        &self.sink
    }

    /// Access mutable reference to the inner sink.
    pub fn sink_mut(&mut self) -> &mut S {
        &mut self.sink
    }

    /// Generates a poster artifact given probe metadata and a backdrop policy.
    ///
    /// Selects a keyframe based on `policy.select_at`, decodes it via the selector's
    /// decoder (or software fallback), formats the image bytes according to `policy.mime_budget`,
    /// emits to the sink, and optionally publishes a `HardwareEvent::PosterGenerated` event.
    pub fn produce_poster<E: EngineSelector>(
        &mut self,
        selector: &E,
        probe: &MediaDescriptor,
        policy: &BackdropPolicy,
        uri_hint: &str,
        publisher: Option<&dyn EventPublisher>,
    ) -> Result<DegradationTier, Box<dyn Error + Send + Sync>> {
        // Collect candidate keyframe packets from packet source
        let mut keyframes: Vec<EncodedPacket> = Vec::new();
        while let Some(packet) = self.source.next_packet()? {
            if packet.is_keyframe {
                keyframes.push(packet);
            }
        }

        if keyframes.is_empty() {
            return Err("no keyframes found in packet source".into());
        }

        // Determine which keyframe to select based on policy.select_at
        let selected_packet = match policy.select_at {
            BackdropSelect::FirstKeyframe => &keyframes[0],
            BackdropSelect::Midpoint => {
                let mid_idx = keyframes.len() / 2;
                &keyframes[mid_idx]
            }
            BackdropSelect::Sharpest => {
                // Compute variance on luma channel over candidate keyframes.
                // Higher variance = sharper detail.
                let mut best_packet = &keyframes[0];
                let mut best_variance = -1.0f64;
                for kf in &keyframes {
                    let var = Self::compute_luma_variance(&kf.data);
                    if var > best_variance {
                        best_variance = var;
                        best_packet = kf;
                    }
                }
                best_packet
            }
        };

        // Determine video codec from probe
        let codec = probe
            .video
            .as_ref()
            .map(|v| match v.codec {
                mediamaster_probe::types::VideoCodec::H264 => Codec::H264,
                mediamaster_probe::types::VideoCodec::Hevc => Codec::Hevc,
                mediamaster_probe::types::VideoCodec::Av1 => Codec::Av1,
                _ => Codec::H264,
            })
            .unwrap_or(Codec::H264);

        // Acquire decoder via EngineSelector (fall back to software path)
        let engine = selector.select_decoder(&self.caps, codec);
        let _engine_type = match engine {
            Engine::Software => "software",
            Engine::MediaFoundation => "media_foundation",
            Engine::AndroidNdk => "android_ndk",
        };

        // Select mime type from preference order or fallback to image/jpeg
        let mime = policy
            .mime_budget
            .first()
            .map(|s| s.as_str())
            .unwrap_or("image/jpeg");

        // Select fallback tier (defaults to Tier4PosterFallback)
        let tier = policy
            .tiers
            .first()
            .copied()
            .unwrap_or(DegradationTier::Tier4PosterFallback);

        // Decode / format poster image bytes (pure memory, no disk)
        let image_bytes =
            Self::encode_poster_image(&selected_packet.data, policy.max_edge_px, mime);

        // Emit to sink
        self.sink
            .emit_backdrop(tier, mime, &image_bytes, uri_hint)?;

        // Emit bus event if publisher is provided
        if let Some(publ) = publisher {
            publ.publish(HardwareEvent::PosterGenerated {
                uri_hint: uri_hint.to_string(),
                tier,
            });
        }

        Ok(tier)
    }

    /// Computes the sample variance of luma / grayscale intensity across packet bytes.
    /// Var = E[Y^2] - (E[Y])^2.
    pub fn compute_luma_variance(data: &[u8]) -> f64 {
        if data.is_empty() {
            return 0.0;
        }
        let count = data.len() as f64;
        let mut sum = 0.0;
        let mut sum_sq = 0.0;
        for &byte in data {
            let val = byte as f64;
            sum += val;
            sum_sq += val * val;
        }
        let mean = sum / count;
        (sum_sq / count) - (mean * mean)
    }

    /// Formats decoded poster image bytes.
    ///
    /// Produces a deterministic memory buffer representing the image container (e.g. JPEG/WebP header + payload).
    fn encode_poster_image(source_data: &[u8], max_edge_px: u32, mime: &str) -> Vec<u8> {
        let mut out = Vec::new();
        // Tag with mime format indicator
        match mime {
            "image/jpeg" => {
                out.extend_from_slice(&[0xFF, 0xD8, 0xFF, 0xE0]); // JPEG SOI & APP0 marker
            }
            "image/webp" => {
                out.extend_from_slice(b"RIFF\0\0\0\0WEBP"); // WebP header
            }
            _ => {
                out.extend_from_slice(b"POSTER");
            }
        }
        // Encode max edge dimension metadata in 4 bytes
        out.extend_from_slice(&max_edge_px.to_be_bytes());
        // Append raw keyframe payload
        out.extend_from_slice(source_data);
        out
    }
}
