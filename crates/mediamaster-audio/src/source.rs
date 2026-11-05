//! Audio source implementations: callback, decoded file streaming, and reshaping.

use mediamaster_bus::EventBus;
use mediamaster_core::error::MediamasterError;
use mediamaster_core::events::HardwareEvent;
use mediamaster_core::traits::AudioSource;
use std::sync::Mutex;

#[derive(Debug, Clone)]
pub struct MediaSourceAdapter {
    /// The wrapped hardware domain source (e.g. `tv_rs` decoder), or a dummy.
    #[allow(dead_code)]
    inner: MediaSource,
    label: String,
}

#[derive(Debug, Clone)]
enum MediaSource {
    Dummy,
}

impl MediaSourceAdapter {
    pub fn dummy(label: impl Into<String>) -> Self {
        Self {
            inner: MediaSource::Dummy,
            label: label.into(),
        }
    }
}

impl AudioSource for MediaSourceAdapter {
    fn read_pcm(&self, buffer: &mut [f32]) -> usize {
        let len = buffer.len().min(256);
        for slot in buffer.iter_mut().take(len) {
            *slot = 0.0;
        }
        len
    }

    fn label(&self) -> &str {
        &self.label
    }
}

#[cfg(feature = "decode")]
pub struct PcmFileSource {
    samples: Vec<f32>,
    cursor: std::sync::atomic::AtomicUsize,
    looping: std::sync::atomic::AtomicBool,
    sample_rate: u32,
    channels: usize,
    label: String,
    event_bus: Option<std::sync::Arc<EventBus>>,
}

#[cfg(feature = "decode")]
impl PcmFileSource {
    /// Load and decode an audio file from the specified path or media source.
    pub fn open_file(
        path: impl AsRef<std::path::Path>,
        label: impl Into<String>,
    ) -> Result<Self, MediamasterError> {
        let file = std::fs::File::open(path.as_ref())
            .map_err(|e| MediamasterError::Audio(format!("failed to open audio file: {}", e)))?;
        let mss = symphonia::core::io::MediaSourceStream::new(Box::new(file), Default::default());
        Self::from_media_source_stream(mss, label)
    }

    /// Decode audio from an in-memory byte slice (e.g., MP3 or WAV bytes).
    pub fn from_bytes(bytes: Vec<u8>, label: impl Into<String>) -> Result<Self, MediamasterError> {
        let cursor = std::io::Cursor::new(bytes);
        let mss = symphonia::core::io::MediaSourceStream::new(Box::new(cursor), Default::default());
        Self::from_media_source_stream(mss, label)
    }

    /// Internal decoder using Symphonia's default probe and codec registry.
    fn from_media_source_stream(
        mss: symphonia::core::io::MediaSourceStream,
        label: impl Into<String>,
    ) -> Result<Self, MediamasterError> {
        use symphonia::core::audio::SampleBuffer;
        use symphonia::core::codecs::DecoderOptions;
        use symphonia::core::formats::FormatOptions;
        use symphonia::core::meta::MetadataOptions;

        let label = label.into();
        let hint = symphonia::core::probe::Hint::new();
        let format_opts = FormatOptions {
            enable_gapless: true,
            ..Default::default()
        };
        let meta_opts = MetadataOptions::default();

        let probed = symphonia::default::get_probe()
            .format(&hint, mss, &format_opts, &meta_opts)
            .map_err(|e| MediamasterError::Audio(format!("failed to probe format: {}", e)))?;

        let mut format = probed.format;
        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
            .ok_or_else(|| MediamasterError::Audio("no audio track found in file".into()))?;

        let track_id = track.id;
        let sample_rate = track.codec_params.sample_rate.ok_or_else(|| {
            MediamasterError::Audio("missing sample rate in track metadata".into())
        })?;
        let channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(2);

        let decoder_opts = DecoderOptions::default();
        let mut decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &decoder_opts)
            .map_err(|e| MediamasterError::Audio(format!("unsupported codec: {}", e)))?;

        let mut samples = Vec::new();
        let mut sample_buf: Option<SampleBuffer<f32>> = None;

        loop {
            let packet = match format.next_packet() {
                Ok(packet) => packet,
                Err(symphonia::core::errors::Error::IoError(ref err))
                    if err.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    break;
                }
                Err(symphonia::core::errors::Error::ResetRequired) => {
                    decoder.reset();
                    continue;
                }
                Err(e) => {
                    // Check if it's end of stream
                    let msg = e.to_string();
                    if msg.contains("end of stream") || msg.contains("eof") {
                        break;
                    }
                    return Err(MediamasterError::Audio(format!("demux error: {}", e)));
                }
            };

            if packet.track_id() != track_id {
                continue;
            }

            match decoder.decode(&packet) {
                Ok(audio_buf) => {
                    if sample_buf.is_none() {
                        let spec = *audio_buf.spec();
                        let duration = audio_buf.capacity() as u64;
                        sample_buf = Some(SampleBuffer::new(duration, spec));
                    }

                    if let Some(buf) = sample_buf.as_mut() {
                        buf.copy_interleaved_ref(audio_buf);
                        samples.extend_from_slice(buf.samples());
                    }
                }
                Err(symphonia::core::errors::Error::DecodeError(msg)) => {
                    eprintln!("symphonia decode error (skipping packet): {}", msg);
                    continue;
                }
                Err(symphonia::core::errors::Error::ResetRequired) => {
                    decoder.reset();
                    continue;
                }
                Err(e) => {
                    return Err(MediamasterError::Audio(format!("decode error: {}", e)));
                }
            }
        }

        if samples.is_empty() {
            return Err(MediamasterError::Audio(
                "decoded file produced zero samples".into(),
            ));
        }

        Ok(Self {
            samples,
            cursor: std::sync::atomic::AtomicUsize::new(0),
            looping: std::sync::atomic::AtomicBool::new(false),
            sample_rate,
            channels,
            label,
            event_bus: None,
        })
    }

    /// Construct directly from raw decoded samples with format properties.
    pub fn from_raw_samples(
        samples: Vec<f32>,
        sample_rate: u32,
        channels: usize,
        label: impl Into<String>,
    ) -> Result<Self, MediamasterError> {
        let label = label.into();
        if label.is_empty() {
            return Err(MediamasterError::Audio(
                "source label cannot be empty".into(),
            ));
        }
        if channels == 0 {
            return Err(MediamasterError::Audio("channels must be >= 1".into()));
        }
        if samples.is_empty() {
            return Err(MediamasterError::Audio("samples cannot be empty".into()));
        }
        Ok(Self {
            samples,
            cursor: std::sync::atomic::AtomicUsize::new(0),
            looping: std::sync::atomic::AtomicBool::new(false),
            sample_rate,
            channels,
            label,
            event_bus: None,
        })
    }

    /// Attach an [`EventBus`] to emit underrun/overrun events.
    pub fn with_event_bus(mut self, bus: std::sync::Arc<EventBus>) -> Self {
        self.event_bus = Some(bus);
        self
    }

    /// Enable or disable looping.
    pub fn set_looping(&self, looping: bool) {
        self.looping
            .store(looping, std::sync::atomic::Ordering::Relaxed);
    }

    /// Returns whether looping is enabled.
    pub fn is_looping(&self) -> bool {
        self.looping.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Seek to a sample offset in the decoded buffer.
    pub fn seek_sample(&self, sample_idx: usize) {
        let target = sample_idx.min(self.samples.len());
        self.cursor
            .store(target, std::sync::atomic::Ordering::Relaxed);
    }

    /// Seek to a time in milliseconds.
    pub fn seek_ms(&self, ms: u64) {
        let frames = (ms * self.sample_rate as u64) / 1000;
        let samples = (frames as usize).saturating_mul(self.channels);
        self.seek_sample(samples);
    }

    /// Total number of samples in the stream.
    pub fn total_samples(&self) -> usize {
        self.samples.len()
    }

    /// Sample rate in Hz.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Channel count.
    pub fn channels(&self) -> usize {
        self.channels
    }
}

#[cfg(feature = "decode")]
impl AudioSource for PcmFileSource {
    fn read_pcm(&self, buffer: &mut [f32]) -> usize {
        if buffer.is_empty() || self.samples.is_empty() {
            return 0;
        }

        let is_looping = self.looping.load(std::sync::atomic::Ordering::Relaxed);
        let mut written = 0;

        while written < buffer.len() {
            let cur = self.cursor.load(std::sync::atomic::Ordering::Relaxed);
            if cur >= self.samples.len() {
                if is_looping {
                    self.cursor.store(0, std::sync::atomic::Ordering::Relaxed);
                    continue;
                } else {
                    // Reached end of file: emit AudioUnderrun if we couldn't fulfill the entire buffer
                    let missing = buffer.len() - written;
                    if let Some(bus) = &self.event_bus {
                        let _ = bus.publish(HardwareEvent::AudioUnderrun {
                            source: self.label.clone(),
                            dropped_frames: missing / self.channels.max(1),
                        });
                    }
                    for slot in &mut buffer[written..] {
                        *slot = 0.0;
                    }
                    return written;
                }
            }

            let available = self.samples.len() - cur;
            let needed = buffer.len() - written;
            let to_copy = available.min(needed);

            buffer[written..written + to_copy].copy_from_slice(&self.samples[cur..cur + to_copy]);
            self.cursor
                .store(cur + to_copy, std::sync::atomic::Ordering::Relaxed);
            written += to_copy;
        }

        written
    }

    fn label(&self) -> &str {
        &self.label
    }
}

/// Adapter that resamples and/or remaps channels from an upstream [`AudioSource`].
pub struct ReshapingSource<S: AudioSource> {
    inner: S,
    label: String,
    source_rate: u32,
    target_rate: u32,
    source_channels: usize,
    target_channels: usize,
    event_bus: Option<std::sync::Arc<EventBus>>,
    state: Mutex<ReshapingState>,
}

struct ReshapingState {
    #[cfg(feature = "resample")]
    resampler: Option<rubato::Fft<f32>>,
    #[cfg(feature = "resample")]
    in_buf: Option<rubato::audioadapter_buffers::owned::InterleavedOwned<f32>>,
    #[cfg(feature = "resample")]
    out_buf: Option<rubato::audioadapter_buffers::owned::InterleavedOwned<f32>>,
    // Ring buffer storing target-format samples ready for reading
    fifo: ringbuf::HeapRb<f32>,
    // Pre-allocated scratch buffer for reading from upstream inner source
    source_scratch: Vec<f32>,
    // Pre-allocated scratch buffer for channel remapping before resampling
    remapped_scratch: Vec<f32>,
}

impl<S: AudioSource> ReshapingSource<S> {
    /// Create a new `ReshapingSource` converting from `(source_rate, source_channels)`
    /// to `(target_rate, target_channels)`.
    pub fn new(
        inner: S,
        source_rate: u32,
        target_rate: u32,
        source_channels: usize,
        target_channels: usize,
    ) -> Result<Self, MediamasterError> {
        if source_rate == 0 || target_rate == 0 {
            return Err(MediamasterError::Audio(
                "sample rate must be non-zero".into(),
            ));
        }
        if source_channels == 0 || target_channels == 0 {
            return Err(MediamasterError::Audio(
                "channel counts must be non-zero".into(),
            ));
        }

        let label = format!("{}-reshaped", inner.label());

        #[cfg(feature = "resample")]
        let (resampler, in_buf, out_buf, in_frames, out_frames) = if source_rate != target_rate {
            use rubato::{Fft, FixedSync, Resampler};
            let r = Fft::<f32>::new(
                source_rate as usize,
                target_rate as usize,
                1024,
                target_channels,
                FixedSync::Input,
            )
            .map_err(|e| MediamasterError::Audio(format!("failed to create resampler: {:?}", e)))?;
            let in_max = r.input_frames_max();
            let out_max = r.output_frames_max();
            let in_b = rubato::audioadapter_buffers::owned::InterleavedOwned::<f32>::new(
                0.0,
                target_channels,
                in_max,
            );
            let out_b = rubato::audioadapter_buffers::owned::InterleavedOwned::<f32>::new(
                0.0,
                target_channels,
                out_max,
            );
            (Some(r), Some(in_b), Some(out_b), in_max, out_max)
        } else {
            (None, None, None, 1024, 1024)
        };

        #[cfg(not(feature = "resample"))]
        if source_rate != target_rate {
            return Err(MediamasterError::Audio(
                "resampling requires enabling feature 'resample'".into(),
            ));
        }

        #[cfg(not(feature = "resample"))]
        let (in_frames, out_frames) = (1024, 1024);

        let fifo_cap = (out_frames * target_channels * 8).max(4096);
        let source_scratch_len = in_frames * source_channels;
        let remapped_scratch_len = in_frames * target_channels;

        let state = ReshapingState {
            #[cfg(feature = "resample")]
            resampler,
            #[cfg(feature = "resample")]
            in_buf,
            #[cfg(feature = "resample")]
            out_buf,
            fifo: ringbuf::HeapRb::new(fifo_cap),
            source_scratch: vec![0.0f32; source_scratch_len],
            remapped_scratch: vec![0.0f32; remapped_scratch_len],
        };

        Ok(Self {
            inner,
            label,
            source_rate,
            target_rate,
            source_channels,
            target_channels,
            event_bus: None,
            state: Mutex::new(state),
        })
    }

    /// Attach an [`EventBus`] to emit buffer events.
    pub fn with_event_bus(mut self, bus: std::sync::Arc<EventBus>) -> Self {
        self.event_bus = Some(bus);
        self
    }

    /// Source sample rate in Hz.
    pub fn source_rate(&self) -> u32 {
        self.source_rate
    }

    /// Target sample rate in Hz.
    pub fn target_rate(&self) -> u32 {
        self.target_rate
    }

    /// Source channels.
    pub fn source_channels(&self) -> usize {
        self.source_channels
    }

    /// Target channels.
    pub fn target_channels(&self) -> usize {
        self.target_channels
    }
}

/// Helper to remap channel frames without allocating
fn remap_channels(
    input: &[f32],
    in_channels: usize,
    output: &mut [f32],
    out_channels: usize,
    frames: usize,
) {
    if in_channels == out_channels {
        output[..frames * out_channels].copy_from_slice(&input[..frames * in_channels]);
        return;
    }

    for f in 0..frames {
        if in_channels == 1 && out_channels == 2 {
            // Mono -> Stereo: duplicate channel
            let s = input[f];
            output[f * 2] = s;
            output[f * 2 + 1] = s;
        } else if in_channels == 2 && out_channels == 1 {
            // Stereo -> Mono: average channels
            let left = input[f * 2];
            let right = input[f * 2 + 1];
            output[f] = 0.5 * (left + right);
        } else {
            // General N -> M
            for c in 0..out_channels {
                if c < in_channels {
                    output[f * out_channels + c] = input[f * in_channels + c];
                } else {
                    output[f * out_channels + c] = 0.0;
                }
            }
        }
    }
}

impl<S: AudioSource> AudioSource for ReshapingSource<S> {
    fn read_pcm(&self, buffer: &mut [f32]) -> usize {
        use ringbuf::Rb;

        if buffer.is_empty() {
            return 0;
        }

        let mut state = match self.state.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };

        let mut read_count = 0;

        while read_count < buffer.len() {
            // Pop as many available samples as possible from the FIFO
            while read_count < buffer.len() && state.fifo.len() > 0 {
                if let Some(s) = state.fifo.pop() {
                    buffer[read_count] = s;
                    read_count += 1;
                }
            }

            if read_count == buffer.len() {
                break;
            }

            // Need more samples from upstream
            let needed_frames = (state.source_scratch.len() / self.source_channels).max(1);
            let upstream_samples = needed_frames * self.source_channels;
            let read_up = self
                .inner
                .read_pcm(&mut state.source_scratch[..upstream_samples]);

            if read_up == 0 {
                // Upstream reached EOF or underrun
                if let Some(bus) = &self.event_bus {
                    let _ = bus.publish(HardwareEvent::AudioUnderrun {
                        source: self.label.clone(),
                        dropped_frames: (buffer.len() - read_count) / self.target_channels,
                    });
                }
                for slot in &mut buffer[read_count..] {
                    *slot = 0.0;
                }
                return read_count;
            }

            let frames_read = read_up / self.source_channels;
            if frames_read == 0 {
                for slot in &mut buffer[read_count..] {
                    *slot = 0.0;
                }
                return read_count;
            }

            let ReshapingState {
                #[cfg(feature = "resample")]
                ref mut resampler,
                #[cfg(feature = "resample")]
                ref mut in_buf,
                #[cfg(feature = "resample")]
                ref mut out_buf,
                ref mut fifo,
                ref source_scratch,
                ref mut remapped_scratch,
            } = *state;

            // Remap channels into remapped_scratch
            remap_channels(
                &source_scratch[..frames_read * self.source_channels],
                self.source_channels,
                remapped_scratch,
                self.target_channels,
                frames_read,
            );

            #[cfg(feature = "resample")]
            if let (Some(r), Some(in_b), Some(out_b)) =
                (resampler.as_mut(), in_buf.as_mut(), out_buf.as_mut())
            {
                use rubato::audioadapter::{Adapter, AdapterMut};
                use rubato::Resampler;

                let in_frames = r.input_frames_next();
                let remapped_count = frames_read * self.target_channels;
                let copy_count = remapped_count.min(in_frames * self.target_channels);
                for f in 0..in_frames {
                    for ch in 0..self.target_channels {
                        let idx = f * self.target_channels + ch;
                        let val = if idx < copy_count {
                            remapped_scratch[idx]
                        } else {
                            0.0
                        };
                        let _ = in_b.write_sample(ch, f, &val);
                    }
                }

                let indexing = rubato::Indexing {
                    partial_len: if frames_read < in_frames {
                        Some(frames_read)
                    } else {
                        None
                    },
                    ..Default::default()
                };

                if let Ok((_in_processed, out_produced)) =
                    r.process_into_buffer(in_b, out_b, Some(&indexing))
                {
                    for f in 0..out_produced {
                        for ch in 0..self.target_channels {
                            if let Some(sample) = out_b.read_sample(ch, f) {
                                if fifo.len() == fifo.capacity() {
                                    let _ = fifo.pop();
                                }
                                let _ = fifo.push(sample);
                            }
                        }
                    }
                }
            } else {
                // No resampling needed: push remapped samples straight into fifo
                let remapped_slice = &remapped_scratch[..frames_read * self.target_channels];
                for &sample in remapped_slice {
                    if fifo.len() == fifo.capacity() {
                        let _ = fifo.pop();
                    }
                    let _ = fifo.push(sample);
                }
            }

            #[cfg(not(feature = "resample"))]
            {
                let remapped_slice = &remapped_scratch[..frames_read * self.target_channels];
                for &sample in remapped_slice {
                    if fifo.len() == fifo.capacity() {
                        let _ = fifo.pop();
                    }
                    let _ = fifo.push(sample);
                }
            }
        }

        read_count
    }

    fn label(&self) -> &str {
        &self.label
    }
}

/// A source backed by a closure/callback `FnMut(&mut [f32]) + Send`.
///
/// Lets embedders (cpal capture, mic, network jitter buffer output) push or
/// generate frames on demand without owning the mixer.
pub struct CallbackSource<F> {
    callback: Mutex<F>,
    label: String,
    event_bus: Option<std::sync::Arc<EventBus>>,
}

impl<F: FnMut(&mut [f32]) + Send> CallbackSource<F> {
    /// Create a new `CallbackSource` with the given label and callback closure.
    pub fn new(label: impl Into<String>, callback: F) -> Result<Self, MediamasterError> {
        let label = label.into();
        if label.is_empty() {
            return Err(MediamasterError::Audio(
                "source label cannot be empty".into(),
            ));
        }
        Ok(Self {
            callback: Mutex::new(callback),
            label,
            event_bus: None,
        })
    }

    /// Attach an [`EventBus`] to emit buffer events.
    pub fn with_event_bus(mut self, bus: std::sync::Arc<EventBus>) -> Self {
        self.event_bus = Some(bus);
        self
    }

    /// Notify an underrun or overrun event via the attached event bus.
    pub fn emit_event(&self, event: HardwareEvent) {
        if let Some(bus) = &self.event_bus {
            let _ = bus.publish(event);
        }
    }
}

impl<F: FnMut(&mut [f32]) + Send> AudioSource for CallbackSource<F> {
    fn read_pcm(&self, buffer: &mut [f32]) -> usize {
        if buffer.is_empty() {
            return 0;
        }
        if let Ok(mut cb) = self.callback.lock() {
            cb(buffer);
            buffer.len()
        } else {
            // Poisoned mutex: emit underrun if event bus attached and fill with silence.
            if let Some(bus) = &self.event_bus {
                let _ = bus.publish(HardwareEvent::AudioUnderrun {
                    source: self.label.clone(),
                    dropped_frames: buffer.len(),
                });
            }
            for s in buffer.iter_mut() {
                *s = 0.0;
            }
            buffer.len()
        }
    }

    fn label(&self) -> &str {
        &self.label
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_source_fills_buffer() {
        let mut count = 0.0f32;
        let source = CallbackSource::new("cb", move |buf: &mut [f32]| {
            for slot in buf.iter_mut() {
                *slot = count;
                count += 1.0;
            }
        })
        .unwrap();

        let mut buf = [0.0f32; 4];
        let n = source.read_pcm(&mut buf);
        assert_eq!(n, 4);
        assert_eq!(buf, [0.0, 1.0, 2.0, 3.0]);

        let n = source.read_pcm(&mut buf);
        assert_eq!(n, 4);
        assert_eq!(buf, [4.0, 5.0, 6.0, 7.0]);
    }

    #[cfg(feature = "decode")]
    #[test]
    fn pcm_file_source_reads_loops_and_seeks() {
        let raw = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let file_src = PcmFileSource::from_raw_samples(raw, 48_000, 2, "test_file").unwrap();
        assert_eq!(file_src.sample_rate(), 48_000);
        assert_eq!(file_src.channels(), 2);
        assert_eq!(file_src.total_samples(), 6);

        let mut buf = [0.0f32; 4];
        let read = file_src.read_pcm(&mut buf);
        assert_eq!(read, 4);
        assert_eq!(buf, [1.0, 2.0, 3.0, 4.0]);

        // Next 4: only 2 remain, tail padded with 0.0
        let read = file_src.read_pcm(&mut buf);
        assert_eq!(read, 2);
        assert_eq!(buf[0], 5.0);
        assert_eq!(buf[1], 6.0);
        assert_eq!(buf[2], 0.0);
        assert_eq!(buf[3], 0.0);

        // Turn on looping and seek back to 2
        file_src.set_looping(true);
        assert!(file_src.is_looping());
        file_src.seek_sample(2);

        let read = file_src.read_pcm(&mut buf);
        assert_eq!(read, 4);
        assert_eq!(buf, [3.0, 4.0, 5.0, 6.0]);

        // When looping, reads wrap around
        let read = file_src.read_pcm(&mut buf);
        assert_eq!(read, 4);
        assert_eq!(buf, [1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn reshaping_source_remaps_channels() {
        // Mono to stereo reshaping without rate change
        let mut count = 1.0f32;
        let mono_src = CallbackSource::new("mono", move |buf: &mut [f32]| {
            for s in buf.iter_mut() {
                *s = count;
                count += 1.0;
            }
        })
        .unwrap();

        let reshaper = ReshapingSource::new(mono_src, 48_000, 48_000, 1, 2).unwrap();
        let mut out = [0.0f32; 4];
        let read = reshaper.read_pcm(&mut out);
        assert_eq!(read, 4);
        // Frame 0: mono 1.0 -> [1.0, 1.0]
        // Frame 1: mono 2.0 -> [2.0, 2.0]
        assert_eq!(out, [1.0, 1.0, 2.0, 2.0]);
    }

    #[cfg(feature = "resample")]
    #[test]
    fn reshaping_source_resamples_rate() {
        use std::sync::atomic::{AtomicU64, Ordering};

        struct SineSrc {
            freq: f32,
            rate: f32,
            frame: AtomicU64,
        }
        impl AudioSource for SineSrc {
            fn read_pcm(&self, buffer: &mut [f32]) -> usize {
                let start = self.frame.fetch_add(buffer.len() as u64, Ordering::Relaxed);
                let delta = self.freq / self.rate * 2.0 * std::f32::consts::PI;
                for (i, s) in buffer.iter_mut().enumerate() {
                    let phase = (start + i as u64) as f32 * delta;
                    *s = phase.sin();
                }
                buffer.len()
            }
            fn label(&self) -> &str {
                "sine"
            }
        }

        // Resample 48k mono 1000Hz sine wave to 44.1k mono
        let sine = SineSrc {
            freq: 1000.0,
            rate: 48_000.0,
            frame: AtomicU64::new(0),
        };
        let reshaper = ReshapingSource::new(sine, 48_000, 44_100, 1, 1).unwrap();

        // Discard initial transient/filter delay
        let mut warmup = [0.0f32; 4096];
        reshaper.read_pcm(&mut warmup);

        // Read steady-state block
        let mut block = [0.0f32; 2048];
        reshaper.read_pcm(&mut block);

        // Verify amplitude preserved: peak should be close to 1.0 (within 0.05)
        let peak = block.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(
            (peak - 1.0).abs() < 0.05,
            "resampled peak was {peak}, expected close to 1.0"
        );
    }
}
