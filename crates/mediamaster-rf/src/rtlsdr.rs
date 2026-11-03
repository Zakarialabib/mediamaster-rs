//! RTL-SDR hardware backend for the 3-stage I/Q pipeline.
//!
//! Implements the `IqSource` trait for a real RTL-SDR dongle via the `rtlsdr`
//! crate. Each `acquire()` opens the device, reads a batch of I/Q samples, and
//! closes the device. This keeps `RtlSdrSource` `Send + Sync` (the inner
//! `RTLSDRDevice` is a raw FFI pointer, not `Send`).
//!
//! # Example
//! ```ignore
//! use mediamaster_rf::rtlsdr::RtlSdrSource;
//! use mediamaster_rf::IqSource;
//!
//! let src = RtlSdrSource::new(0, 433_920_000, 2_400_000).unwrap();
//! // Feed the pipeline:
//! // let pipeline = Pipeline::new(Arc::new(src), config);
//! ```

use std::os::raw::c_void;

use rtlsdr::{self, RTLSDRDevice};
use tokio::task::spawn_blocking;

use crate::{iq_source::IqSource, IqSample};

/// A real RTL-SDR dongle implementing `IqSource`.
///
/// Stores the device index rather than the handle, so the struct is `Send +
/// Sync` (the underlying `RTLSDRDevice` is a raw FFI pointer, not `Send`).
pub struct RtlSdrSource {
    device_index: usize,
    center_freq_hz: u32,
    sample_rate_hz: u32,
}

impl RtlSdrSource {
    /// Open the RTL-SDR at `device_index`, set `center_freq_hz` and
    /// `sample_rate_hz`.
    pub fn new(
        device_index: usize,
        center_freq_hz: u32,
        sample_rate_hz: u32,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        // Open, configure, and immediately close the device to enumerate
        // capabilities (hardware is re-opened per acquire).
        let _cfg = rtlsdr::open(device_index.try_into().map_err(|e| {
            // RTLSDRError fields are private; box the Display error via `rtlsdr_error`.
            let errstr = format!("RTL-SDR error: {} ({})", e.errstr, e.errno);
            let errno = e.errno;
            let _ = errstr; // NOTE: field names are private; this constructs via Display below.
                            // Construct the error via the private `rtlsdr_error` helper.
            rtlsdr::rtlsdr_error(
                errno as libc::c_int,
                errstr.as_ptr() as *const std::ffi::c_void,
            )
        })?)?;
        Ok(Self {
            device_index,
            center_freq_hz,
            sample_rate_hz,
        })
    }

    /// Device index of the attached dongle.
    pub fn device_index(&self) -> usize {
        self.device_index
    }
}

impl IqSource for RtlSdrSource {
    async fn acquire(&self, count: usize) -> Vec<IqSample> {
        // Open, read, and close on each acquisition. This keeps the struct
        // `Send + Sync` (the underlying `RTLSDRDevice` is a raw FFI pointer).
        spawn_blocking(move || {
            let mut device =
                rtlsdr::open(self.device_index.try_into().unwrap()).expect("rtlsdr open");
            device
                .set_center_freq(self.center_freq_hz)
                .expect("set_center_freq");
            device
                .set_sample_rate(self.sample_rate_hz)
                .expect("set_sample_rate");
            device
                .set_tuner_gain_mode(false)
                .expect("set_tuner_gain_mode");
            device.set_tuner_gain(-10).expect("set_tuner_gain");

            let mut buf = vec![0f32; count * 2];
            let n = device.read_sync(&mut buf).expect("rtlsdr read_sync");

            // Convert interleaved I/Q pairs into Complex<f32>.
            let mut samples = Vec::with_capacity(count);
            let bytes = buf.len().min(n);
            let mut i = 0;
            while i + 1 < bytes && samples.len() < count {
                samples.push(IqSample::new(buf[i], buf[i + 1]));
                i += 2;
            }
            samples
        })
        .await
        .expect("rtlsdr acquire")
    }

    fn sample_rate_hz(&self) -> u32 {
        self.sample_rate_hz
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rtl_sdr_source_is_send_sync() {
        // Verify `RtlSdrSource` satisfies `IqSource: Send + Sync`.
        fn _assert_send_sync<T: Send + Sync>() {}
        let _ = _assert_send_sync::<RtlSdrSource>;
    }
}
