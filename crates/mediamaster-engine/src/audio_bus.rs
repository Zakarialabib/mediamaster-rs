//! `AudioBus` — lock-free PCM ring (SPSC) with ducking + underrun/overflow
//! accounting.
//!
//! A producer (the coordination plane: agent / ducking loops) pushes PCM samples
//! here; a consumer (transcode worker / playback sink) reads them, optionally with
//! a ducking multiplier applied.  The ring is SPSC-atomic (`ringbuf::SharedRb`),
//! so a producer and a consumer may live on different threads.
//!
//! # Design
//!
//! - **`push`**: producer path — appends a sample (no ducking applied, per the
//!   producer's own gain).
//! - **`try_read`**: consumer path — pops a sample and applies the current
//!   ducking gain.  When the ring is empty the consumer is deemed "lagged/underrun";
//!   the miss is counted and `None` is returned (the consumer should hold steady
//!   rather than stall).
//!
//! # Safety
//!
//! `SharedRb<f32>` is `Send + Sync`; an `AudioBus` may be shared across tasks
//! safely.  Callers must not mix `push` (producer) with `try_read` on a single
//! thread, and must not call `try_read` concurrently with `push` from the same
//! thread.

use ringbuf::Rb;
use ringbuf::SharedRb;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct AudioBusConfig {
    /// Ring capacity in samples (≈ 85 ms of 48 kHz mono headroom). Pick a value
    /// so the consumer always has ≥ 1 window of slack before it starves.
    pub capacity: usize,
    /// Initial ducking gain (1.0 = full volume, 0.0 = fully muted).
    pub initial_ducking: f32,
}

pub struct AudioBus {
    ring: SharedRb<f32>,
    /// Current global ducking gain.
    ducking: f32,
    /// Number of consumer underruns (missed frames).  Advisory; reset before
    /// latency-critical phases.
    pub underrun: AtomicUsize,
}

impl AudioBus {
    pub fn new(config: AudioBusConfig) -> Self {
        AudioBus {
            ring: SharedRb::new(config.capacity),
            ducking: config.initial_ducking,
            underrun: AtomicUsize::new(0),
        }
    }

    pub fn ducking(&self) -> f32 {
        self.ducking
    }

    pub fn set_ducking(&self, gain: f32) {
        self.ducking = gain.clamp(0.0, 1.0);
    }

    /// Push one sample; called by the producer (coordination plane).
    pub fn push(&self, sample: f32) {
        self.ring.push(sample);
    }

    /// Push a slice of samples; called by the producer.
    pub fn push_slice(&self, samples: &[f32]) {
        for &s in samples {
            self.ring.push(s);
        }
    }

    /// Try to read one sample for the consumer.
    ///
    /// Applies the current ducking gain to the returned value.  Returns `None`
    /// when the ring is empty (consumer underrun).
    pub fn try_read(&self) -> Option<f32> {
        if self.ring.len() == 0 {
            self.underrun.fetch_add(1, Ordering::Relaxed);
            return None;
        }
        Some(self.ring.pop().unwrap_or(0.0) * self.ducking)
    }

    /// Peek into the ring without consuming.  Returns `None` if empty.
    pub fn try_peek(&self) -> Option<f32> {
        self.ring.peek()
    }

    /// Current fill ratio (0.0–1.0).
    pub fn fill_ratio(&self) -> f64 {
        self.ring.len() as f64 / self.ring.capacity() as f64
    }

    /// Number of underrun misses.
    pub fn underrun_count(&self) -> usize {
        self.underrun.load(Ordering::Relaxed)
    }
}

impl Default for AudioBus {
    fn default() -> Self {
        Self::new(AudioBusConfig {
            capacity: 4096,
            initial_ducking: 1.0,
        })
    }
}
