#[cfg(feature = "opus")]
mod opus_tests {
    use mediamaster_audio::{OpusPagingSource, OPUS_FRAME_SAMPLES_20MS, OPUS_SAMPLE_RATE};
    use mediamaster_core::traits::AudioSource;
    use std::f32::consts::PI;

    fn generate_sine_wave(sample_rate: u32, freq_hz: f32, num_samples: usize) -> Vec<f32> {
        (0..num_samples)
            .map(|i| {
                let t = i as f32 / sample_rate as f32;
                (2.0 * PI * freq_hz * t).sin() * 0.7
            })
            .collect()
    }

    fn calculate_rms(samples: &[f32]) -> f32 {
        if samples.is_empty() {
            return 0.0;
        }
        let sum_sq: f32 = samples.iter().map(|&s| s * s).sum();
        (sum_sq / samples.len() as f32).sqrt()
    }

    #[test]
    fn opus_round_trip_with_15_percent_loss_and_reordering() {
        let sample_rate = OPUS_SAMPLE_RATE;
        let frame_size = OPUS_FRAME_SAMPLES_20MS; // 960 samples = 20ms at 48kHz
        let total_frames = 100; // 2 seconds of 440Hz sine wave audio
        let input_pcm = generate_sine_wave(sample_rate, 440.0, total_frames * frame_size);
        let original_rms = calculate_rms(&input_pcm);
        assert!(
            original_rms > 0.4 && original_rms < 0.6,
            "original RMS was {original_rms}"
        );

        // Create Opus encoder
        let mut encoder =
            opus::Encoder::new(sample_rate, opus::Channels::Mono, opus::Application::Voip)
                .expect("failed to create opus encoder");

        // Encode all frames
        let mut encoded_packets: Vec<(u64, Vec<u8>)> = Vec::new();
        let mut packet_buf = vec![0u8; 1024];
        for i in 0..total_frames {
            let start = i * frame_size;
            let end = start + frame_size;
            let frame = &input_pcm[start..end];
            let len = encoder
                .encode_float(frame, &mut packet_buf)
                .expect("opus encode failed");
            encoded_packets.push((i as u64, packet_buf[..len].to_vec()));
        }

        // Simulate 15% packet loss deterministically (e.g., indices where i % 7 == 0: ~14.3%)
        let mut network_packets: Vec<(u64, Vec<u8>)> = Vec::new();
        let mut lost_count = 0;
        for (seq, payload) in encoded_packets {
            if seq > 5 && seq % 7 == 0 {
                // Drop packet to simulate network loss
                lost_count += 1;
            } else {
                network_packets.push((seq, payload));
            }
        }
        assert!(
            lost_count >= 13 && lost_count <= 16,
            "simulated loss count was {lost_count}"
        );

        // Simulate network packet reordering on a subset of frames
        // Swap packet at index 10 and 11
        if network_packets.len() > 15 {
            network_packets.swap(10, 11);
            network_packets.swap(20, 21);
        }

        // Create OpusPagingSource with 50ms prebuffer
        let paging_source = OpusPagingSource::with_jitter_config("ptt-mic", sample_rate, 50)
            .expect("failed to create OpusPagingSource");

        // Stream packets and read concurrently to simulate real-time playback
        // (rather than dumping 100 packets into a 4096-capacity jitter buffer at once!)
        let mut decoded_pcm = vec![0.0f32; total_frames * frame_size];
        let mut read_total = 0;
        let chunk_size = 480; // 10ms read chunks

        // Feed packets gradually while draining
        let mut packet_iter = network_packets.into_iter();
        while read_total < decoded_pcm.len() {
            // Push 1 packet every 20ms of read audio (2 * chunk_size)
            if let Some((seq, payload)) = packet_iter.next() {
                paging_source.push_packet(seq, payload);
            }

            let end = (read_total + chunk_size).min(decoded_pcm.len());
            let n = paging_source.read_pcm(&mut decoded_pcm[read_total..end]);
            read_total += n;
            if n == 0 {
                break;
            }
        }

        // Verify output is non-empty and contains valid audio (finite values)
        assert!(
            decoded_pcm.iter().all(|s| s.is_finite()),
            "NaN or Inf in decoded stream"
        );

        // Inspect decoded RMS after jitter prebuffer priming (skip initial 100ms = 4800 samples)
        let active_slice = &decoded_pcm[4800..decoded_pcm.len() - 4800];
        let decoded_rms = calculate_rms(active_slice);

        // Even with 15% packet loss, PLC should maintain RMS energy continuity within tolerance
        // compared to original RMS (typically within ~30-40% of original, never collapsing to 0).
        assert!(
            decoded_rms > 0.25,
            "decoded RMS collapsed ({decoded_rms}); PLC did not maintain energy"
        );
        assert!(
            (decoded_rms - original_rms).abs() < 0.25,
            "RMS deviation too high: original={original_rms}, decoded={decoded_rms}"
        );
    }
}
