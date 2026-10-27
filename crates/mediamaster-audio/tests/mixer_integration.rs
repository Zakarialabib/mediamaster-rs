//! Integration test: Mixer consumes MediaSourceAdapter (core trait bridge).

use mediamaster_audio::{MediaSourceAdapter, Mixer};

#[test]
fn mixer_mixes_adapter_silence() {
    let mut mixer = Mixer::new();
    mixer.add_source(Box::new(MediaSourceAdapter::dummy("test")));
    let output = mixer.mix();

    // Adapter produces silence (0.0), so all output samples are 0.0.
    assert!(!output.is_empty());
    assert!(output.iter().all(|s| *s == 0.0));
}
