//! `AudioSource` reference adapter: bridges another hardware domain (e.g.
//! `tv-rs`) into the mediamaster audio mixer.

use mediamaster_core::traits::AudioSource;

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
        Self { inner: MediaSource::Dummy, label: label.into() }
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
