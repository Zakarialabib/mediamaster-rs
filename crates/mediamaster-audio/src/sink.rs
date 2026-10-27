//! OS audio sink: the final output thread reads from a ring buffer and writes
//! to OS audio hardware via `cpal`.
//!
//! Enabled by the `audio-cpal` feature.

#[cfg(feature = "audio-cpal")]
use std::sync::Arc;

// cpal 0.15 trait imports (feature-gated).
#[cfg(feature = "audio-cpal")]
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// The audio output sink. A `cpal` stream callback reads from the output ring
/// and writes to the default output device.
#[cfg(feature = "audio-cpal")]
pub struct AudioSink {
    device: cpal::Device,
    config: cpal::StreamConfig,
}

#[cfg(feature = "audio-cpal")]
impl AudioSink {
    /// Open the default output device.
    pub fn open_default() -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or("no default output device found")?;
        let config = device
            .default_output_config()
            .map_err(|e| format!("failed to get default output config: {}", e))?
            .config();
        Ok(Self { device, config })
    }

    /// Start playback. `reader` is sampled on the cpal audio thread via `Arc`.
    ///
    /// The returned `Stream` must be kept alive for the duration of playback.
    pub fn start<R: LockRead + Send + Sync + 'static>(
        &self,
        reader: Arc<R>,
    ) -> Result<cpal::Stream, Box<dyn std::error::Error>> {
        let err_fn = |err: cpal::StreamError| {
            eprintln!("an audio stream error occurred: {}", err);
        };
        let stream = self.device.build_output_stream(
            &self.config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let n = reader.read_f32(data);
                for slot in data.iter_mut().skip(n) {
                    *slot = 0.0;
                }
            },
            err_fn,
            None, // timeout: no timeout
        )?;
        stream.play()?;
        Ok(stream)
    }
}

/// A read handle that the `cpal` callback acquires without blocking the runtime.
///
/// Implementations must be `Send + Sync` so the cpal audio thread can call
/// `read_f32` concurrently with the producer thread.
pub trait LockRead: Send + Sync {
    /// Fill `buffer` with audio samples, returning the number of frames
    /// written. Unfilled frames are silence.
    fn read_f32(&self, buffer: &mut [f32]) -> usize;
}
