//! Quirks-aware capability discovery adapter.
//!
//! Wraps any backend [`CapsDiscovery`] implementation and overlays empirical
//! quirks and penalties from a [`QuirksCorpus`]. This enforces epistemic
//! honesty: drivers claiming 4K60 or unsupported multi-streams are penalized
//! or downgraded prior to transcode planning or layout activation.

use mediamaster_codec::discovery::CapsDiscovery;
use mediamaster_core::CapsMatrix;

use crate::quirks::QuirksCorpus;

/// Adapter wrapping any [`CapsDiscovery`] backend to overlay known quirks.
#[derive(Debug, Clone)]
pub struct QuirksAwareDiscovery<B: CapsDiscovery> {
    backend: B,
    corpus: QuirksCorpus,
    device_class: String,
}

impl<B: CapsDiscovery> QuirksAwareDiscovery<B> {
    /// Creates a new `QuirksAwareDiscovery` instance.
    pub fn new(backend: B, corpus: QuirksCorpus, device_class: impl Into<String>) -> Self {
        Self {
            backend,
            corpus,
            device_class: device_class.into(),
        }
    }

    /// Access the underlying backend.
    pub fn backend(&self) -> &B {
        &self.backend
    }

    /// Access the quirks corpus.
    pub fn corpus(&self) -> &QuirksCorpus {
        &self.corpus
    }

    /// Access the device class.
    pub fn device_class(&self) -> &str {
        &self.device_class
    }
}

impl<B: CapsDiscovery> CapsDiscovery for QuirksAwareDiscovery<B> {
    fn discover(&self) -> CapsMatrix {
        let mut raw_matrix = self.backend.discover();

        // Overlay quirks on each advertised codec capability
        for codec_caps in &mut raw_matrix.codecs {
            if let Some(adjusted) = self.corpus.query(&self.device_class, codec_caps) {
                *codec_caps = adjusted;
            }
        }

        // Check if there are quirks limiting concurrent instances
        for quirk in self.corpus.entries() {
            if quirk.device_class.eq_ignore_ascii_case(&self.device_class) {
                if let Some(max_streams) = quirk.max_concurrent_streams {
                    if raw_matrix.max_hw_decoder_instances == 0
                        || (max_streams as u8) < raw_matrix.max_hw_decoder_instances
                    {
                        raw_matrix.max_hw_decoder_instances = max_streams as u8;
                    }
                }
            }
        }

        raw_matrix
    }
}
