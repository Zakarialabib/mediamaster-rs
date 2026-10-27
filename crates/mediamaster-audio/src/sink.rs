//! OS audio sink: the final output thread reads from a ring buffer and writes
//! to OS audio hardware via `cpal`.

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
    pub fn open_default() -> Result<Self, cpal::BuildStreamError> {
        let device = cpal::default_device().ok_or(cpal::BuildStreamError::DeviceNotFound)?;
        let config = device.default_output_config()?.config();
        Ok(Self { device, config })
    }

    /// Start playback. `read` is the output ring buffer (locked separately).
    pub fn start<R>(&self, read: &R)
    where
        R: LockRead,
    {
        let config = self.config;
        let _stream = device.build_stream(read, &config).map(|s| s.play());
    }
}

/// A read handle that the `cpal` callback acquires without blocking the runtime.
pub trait LockRead: Send + Sync {
    fn read_f32(&self, buffer: &mut [f32]) -> usize;
}
