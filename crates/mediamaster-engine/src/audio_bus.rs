//! `AudioBus` — lock-free PCM ring buffer between the producer (agent) and
//! the consumer (transcode / playback).  The producer pushes PCM samples here;
//! a consumer reads them and mixes them into the output stream.
//!
//! The bus is an SPSC ring (single producer, single consumer).  The producer
//! must not call `push` concurrently with itself; the consumer must not call
//! `try_read` concurrently with itself; but producer and consumer may run on
//! different threads.
//!
//! The ducking gain is stored alongside the ring and is updated by the
//! coordinator (which holds `&mut AudioBus`).

use ringbuf::HeapRb;
use ringbuf::Rb;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

pub struct AudioBusConfig {
    /// Ring capacity (samples, not bytes).
    pub capacity: usize,
    /// Initial ducking gain (0.0 = silence, 1.0 = full).
    pub initial_ducking: f32,
}

pub struct AudioBus {
    ring: Mutex<HeapRb<f32>>,
    /// Current global ducking gain.
    ducking: f32,
    /// Number of consumer underruns (missed frames).  Advisory; reset before
    /// each run.
    pub underrun: AtomicUsize,
}

impl AudioBus {
    pub fn new(config: AudioBusConfig) -> Self {
        AudioBus {
            ring: Mutex::new(HeapRb::new(config.capacity)),
            ducking: config.initial_ducking,
            underrun: AtomicUsize::new(0),
        }
    }

    /// Push a batch of PCM samples into the ring.  Returns the number of
    /// samples actually written (may be less than `samples.len()` on overflow).
    pub fn push(&self, samples: &[f32]) -> usize {
        let mut ring = self.ring.lock().unwrap();
        let mut written = 0;
        for &s in samples {
            if ring.push(s).is_ok() {
                written += 1;
            } else {
                break;
            }
        }
        written
    }

    /// Try to read the next sample.  Returns `None` on underrun.
    pub fn try_read(&self) -> Option<f32> {
        let mut ring = self.ring.lock().unwrap();
        match ring.pop() {
            Some(sample) => Some(sample * self.ducking),
            None => {
                self.underrun.fetch_add(1, Ordering::Relaxed);
                None
            }
        }
    }

    /// Set the global ducking gain (0.0 = silence, 1.0 = full).
    pub fn set_ducking(&mut self, gain: f32) {
        self.ducking = gain.clamp(0.0, 1.0);
    }

    /// Current ducking gain.
    pub fn ducking(&self) -> f32 {
        self.ducking
    }
}
