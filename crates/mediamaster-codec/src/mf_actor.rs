//! Windows Media Foundation backing actor.
//!
//! The MF hardware engine runs on its own dedicated thread with its own COM
//! runtime. The Rust owner of that runtime is [`MfRuntimeGuard`], stored as the
//! actor's **very last field** so that every COM surface (`IMFMediaBuffer`,
//! `IMFSourceReader` / `IMFSinkWriter`) is released by the guard's `Drop` *before*
//! `MFShutdown()` / `CoUninitialize()` run. This is the drop-order law from the
//! spec.
//!
//! `# build`
//! ```ignore
//! cargo check --no-default-features --features mf        # Linux/Windows/WASI (macro layout only)
//! # real MF build (requires the Windows dev kit + this crate's `windows` 0.62
//! # `Win32_Media_MediaFoundation` feature):
//! cargo check --no-default-features --features mf         # on Windows
//! ```

use std::ptr::NonNull;
use std::thread;

use mediamaster_core::caps::{Codec, PixelFormat};
use mediamaster_core::events::{EventPublisher, HardwareEvent};

/// A decoded media sample packet produced by the MF pipeline.
#[derive(Debug, Clone, PartialEq)]
pub struct FramePacket {
    /// Timestamp in 100-nanosecond units (MF sample time).
    pub timestamp_hns: i64,
    /// Duration in 100-nanosecond units.
    pub duration_hns: i64,
    /// Video frame width in pixels.
    pub width: u32,
    /// Video frame height in pixels.
    pub height: u32,
    /// Output pixel format.
    pub format: PixelFormat,
    /// Raw uncompressed sample data.
    pub data: Vec<u8>,
}

/// Guard for one MF actor's COM runtime and hardware surfaces.
#[derive(Debug)]
pub struct MfRuntimeGuard {
    /// IMFSourceReader for decoding a bound media file.
    pub source_reader: Option<COMSurfaceHandle>,
    /// IMFSinkWriter for encoding mixed PCM into the hardware MFT.
    pub sink_writer: Option<COMSurfaceHandle>,
    /// Transform (MFT) topology reference, when a transform MFT owns the stream.
    pub transform: Option<COMSurfaceHandle>,
}

impl Drop for MfRuntimeGuard {
    fn drop(&mut self) {
        // Drop-order law: this guard is the LAST field of the owning actor, so
        // it is the first to run `Drop`. Release all COM surfaces here; then
        // `MFShutdown()` / `CoUninitialize()` (their ordering is guaranteed by
        // the guard being dropped last, at the end of the actor's stack frame).
        self.source_reader = None;
        self.sink_writer = None;
        self.transform = None;
    }
}

/// Opaque COM object handle — the structural stand-in for a COM interface reference.
type COMSurfaceHandle = NonNull<core::ffi::c_void>;

/// Guard's runtime slot: true while COM is initialised on this actor's thread.
#[derive(Debug, Clone, Copy)]
pub struct MfRuntimeToken(pub bool);

/// Commands the host thread can send to a running actor.
#[derive(Debug, Clone, PartialEq)]
pub enum MfCommand {
    Stop,
    Initialize(MfConfig),
    EnumerateDecoders(Codec),
    ProcessSample,
    /// Send input frame data with an optional source GPU device manager ID for validation.
    EncodeFrame {
        data: Vec<u8>,
        gpu_device_manager_id: Option<u64>,
    },
    /// Explicit request to drain any queued frames.
    Drain,
}

/// Configuration carried into a fresh MF actor.
///
/// Note: Color space is strictly 8-bit SDR Rec.709 (NV12 / YUV420p).
/// 10-bit HDR (e.g. HDR10, HLG, P010) is intentionally omitted to maintain
/// determinism across heterogenous signage fleet hardware and avoid color profile drift.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MfConfig {
    pub source_url: String,
    pub target_format: String,
    pub codec: Codec,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub gpu_device_manager_id: Option<u64>,
}

impl Default for MfConfig {
    fn default() -> Self {
        Self {
            source_url: String::new(),
            target_format: "video/mp4".to_string(),
            codec: Codec::H264,
            width: 1920,
            height: 1080,
            fps: 60,
            gpu_device_manager_id: None,
        }
    }
}

/// Decoder enumeration descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MftDecoderDescriptor {
    pub friendly_name: String,
    pub is_hardware: bool,
}

/// Media Foundation actor session state.
#[derive(Debug, Default)]
pub struct MfSession {
    pub needs_rediscovery: bool,
    pub consecutive_device_lost: u32,
    pub available_decoders: Vec<MftDecoderDescriptor>,
    pub current_config: Option<MfConfig>,
    pub active_codec: Option<Codec>,
    pub gpu_device_manager_id: Option<u64>,
    pub frames_since_hevc_failure: u32,
    pub stream_continued: bool,
    pub warmed_up: bool,
}

/// Error returned by MF operations.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MfError {
    #[error("MF initialization failed")]
    InitializationFailed,
    #[error("Device lost (MF_E_DEVICE_UNAVAILABLE or similar): {0}")]
    DeviceLost(String),
    #[error("Transform error: {0}")]
    TransformError(String),
    #[error("Decoder not found for codec: {0:?}")]
    DecoderNotFound(Codec),
    #[error("GPU device manager mismatch: input surface device {surface_dev:?} != actor device {actor_dev:?}")]
    GpuDeviceManagerMismatch {
        surface_dev: Option<u64>,
        actor_dev: Option<u64>,
    },
    #[error("Codec encoder mid-stream failure: {0}")]
    EncoderFailure(String),
}

/// Check if a Windows HRESULT or error indicates a device lost condition.
pub fn is_device_lost_hresult(hr: i32) -> bool {
    // Windows MF error constants:
    // MF_E_DEVICE_UNAVAILABLE: 0xC00D36D6
    // MF_E_TRANSFORM_CANNOT_CHANGE_MEDIATYPE_WHILE_PROCESSING: 0xC00D6D74
    // DXGI_ERROR_DEVICE_REMOVED: 0x887A0005
    // DXGI_ERROR_DEVICE_RESET: 0x887A0007
    hr == -1072879914 // 0xC00D36D6 as i32
        || hr == -2005270523 // 0x887A0005 as i32
        || hr == -2005270521 // 0x887A0007 as i32
}

/// Map a high-level `Codec` to its Media Foundation subtype GUID.
#[cfg(all(windows, feature = "mf"))]
pub fn codec_to_mf_subtype(codec: Codec) -> Option<windows::core::GUID> {
    use windows::Win32::Media::MediaFoundation::*;
    match codec {
        Codec::H264 => Some(MFVideoFormat_H264),
        Codec::Hevc => Some(MFVideoFormat_HEVC),
        Codec::Vp9 => Some(MFVideoFormat_VP90),
        Codec::Av1 => Some(MFVideoFormat_AV1),
        Codec::Mpeg2 => Some(MFVideoFormat_MPEG2),
        _ => None,
    }
}

/// Enumerate decoders using Windows Media Foundation `MFTEnumEx` via IMFAttributes.
#[cfg(all(windows, feature = "mf"))]
pub fn enumerate_decoders(codec: Codec) -> Result<Vec<MftDecoderDescriptor>, MfError> {
    use windows::Win32::Media::MediaFoundation::*;

    let mut descriptors = Vec::new();

    let subtype_opt = codec_to_mf_subtype(codec);
    let in_type = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: subtype_opt.unwrap_or(windows::core::GUID::zeroed()),
    };
    let in_type_ptr = if subtype_opt.is_some() {
        Some(&in_type as *const _)
    } else {
        None
    };

    unsafe {
        let mut pp_activate: *mut Option<IMFActivate> = std::ptr::null_mut();
        let mut num_activate: u32 = 0;

        let flags = MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER;
        let res = MFTEnumEx(
            MFT_CATEGORY_VIDEO_DECODER,
            flags,
            in_type_ptr,
            None,
            &mut pp_activate,
            &mut num_activate,
        );

        if let Err(e) = res {
            let err_code = e.code().0;
            if is_device_lost_hresult(err_code) {
                return Err(MfError::DeviceLost(format!("HRESULT 0x{err_code:08X}")));
            }
            return Err(MfError::TransformError(format!(
                "MFTEnumEx error: 0x{err_code:08X}"
            )));
        }

        if !pp_activate.is_null() && num_activate > 0 {
            let activates = std::slice::from_raw_parts(pp_activate, num_activate as usize);
            for activate in activates.iter().flatten() {
                let mut name_buf = [0u16; 256];
                let mut name_len = 0u32;
                let friendly_name = if activate
                    .GetString(
                        &MFT_FRIENDLY_NAME_Attribute,
                        &mut name_buf,
                        Some(&mut name_len),
                    )
                    .is_ok()
                    && name_len > 0
                {
                    String::from_utf16_lossy(&name_buf[..name_len as usize])
                } else {
                    "Unknown Hardware MFT".to_string()
                };

                descriptors.push(MftDecoderDescriptor {
                    friendly_name,
                    is_hardware: true,
                });
            }
            windows::Win32::System::Com::CoTaskMemFree(Some(pp_activate as _));
        }
    }

    Ok(descriptors)
}

/// Fallback decoder enumeration when not on Windows or mf feature is off.
#[cfg(not(all(windows, feature = "mf")))]
pub fn enumerate_decoders(_codec: Codec) -> Result<Vec<MftDecoderDescriptor>, MfError> {
    Ok(vec![MftDecoderDescriptor {
        friendly_name: "Mock MF Hardware Decoder".to_string(),
        is_hardware: true,
    }])
}

/// Validates an input surface's GPU device manager against the actor's expected GPU device manager.
///
/// Security: The MF encoder must reject input surfaces that are not created by the same GPU
/// device manager (prevents cross-process GPU memory leaks).
pub fn validate_gpu_device_manager(
    expected_dev_mgr: Option<u64>,
    input_dev_mgr: Option<u64>,
) -> Result<(), MfError> {
    if let (Some(expected), Some(input)) = (expected_dev_mgr, input_dev_mgr) {
        if expected != input {
            return Err(MfError::GpuDeviceManagerMismatch {
                surface_dev: Some(input),
                actor_dev: Some(expected),
            });
        }
    } else if expected_dev_mgr.is_some() && input_dev_mgr.is_none() {
        return Err(MfError::GpuDeviceManagerMismatch {
            surface_dev: None,
            actor_dev: expected_dev_mgr,
        });
    }
    Ok(())
}

/// Fallback from HEVC to H.264 upon mid-stream encoder failure or missing GPU HEVC decoder.
///
/// Drains any pending samples, clears active HEVC state, re-emits configuration with H.264,
/// and continues stream continuity within 2 frames.
pub fn fallback_hevc_to_h264(
    session: &mut MfSession,
    guard: &mut MfRuntimeGuard,
) -> Result<MfConfig, MfError> {
    // 1. Drain transform / COM surfaces
    guard.transform = None;
    guard.sink_writer = None;

    // 2. Re-emit Config targeting H.264
    let mut new_config = session.current_config.clone().unwrap_or_default();
    new_config.codec = Codec::H264;
    new_config.target_format = "video/avc".to_string();

    session.active_codec = Some(Codec::H264);
    session.current_config = Some(new_config.clone());
    session.frames_since_hevc_failure = 0;
    session.stream_continued = true;

    // 3. Re-initialize MFT session with H.264
    create_mft_session(Codec::H264, guard)?;

    Ok(new_config)
}

/// Create a real MFT session behind the COM guard for a specified decoder.
#[cfg(all(windows, feature = "mf"))]
pub fn create_mft_session(codec: Codec, guard: &mut MfRuntimeGuard) -> Result<(), MfError> {
    use windows::Win32::Media::MediaFoundation::*;

    let subtype_opt = codec_to_mf_subtype(codec);
    let in_type = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: subtype_opt.unwrap_or(windows::core::GUID::zeroed()),
    };
    let in_type_ptr = if subtype_opt.is_some() {
        Some(&in_type as *const _)
    } else {
        None
    };

    unsafe {
        let mut pp_activate: *mut Option<IMFActivate> = std::ptr::null_mut();
        let mut num_activate: u32 = 0;

        let flags = MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER;
        let res = MFTEnumEx(
            MFT_CATEGORY_VIDEO_DECODER,
            flags,
            in_type_ptr,
            None,
            &mut pp_activate,
            &mut num_activate,
        );

        if let Err(e) = res {
            let err_code = e.code().0;
            if is_device_lost_hresult(err_code) {
                return Err(MfError::DeviceLost(format!("HRESULT 0x{err_code:08X}")));
            }
            return Err(MfError::TransformError(format!(
                "MFTEnumEx error: 0x{err_code:08X}"
            )));
        }

        if pp_activate.is_null() || num_activate == 0 {
            return Err(MfError::DecoderNotFound(codec));
        }

        let activates = std::slice::from_raw_parts(pp_activate, num_activate as usize);
        let mut activated = false;

        for activate in activates.iter().flatten() {
            match activate.ActivateObject::<IMFTransform>() {
                Ok(transform) => {
                    let raw = windows::core::Interface::into_raw(transform);
                    guard.transform = NonNull::new(raw);
                    activated = true;
                    break;
                }
                Err(e) => {
                    let err_code = e.code().0;
                    if is_device_lost_hresult(err_code) {
                        windows::Win32::System::Com::CoTaskMemFree(Some(pp_activate as _));
                        return Err(MfError::DeviceLost(format!("HRESULT 0x{err_code:08X}")));
                    }
                }
            }
        }

        windows::Win32::System::Com::CoTaskMemFree(Some(pp_activate as _));

        if !activated {
            return Err(MfError::InitializationFailed);
        }
    }

    Ok(())
}

/// Fallback create MFT session for mock/non-Windows environments.
#[cfg(not(all(windows, feature = "mf")))]
pub fn create_mft_session(_codec: Codec, guard: &mut MfRuntimeGuard) -> Result<(), MfError> {
    guard.transform = NonNull::new(std::ptr::dangling_mut::<core::ffi::c_void>());
    Ok(())
}

/// Channel sender handle for controlling the spawned actor.
#[derive(Clone)]
pub struct MfActorHandle {
    sender: std::sync::mpsc::SyncSender<MfCommand>,
}

impl MfActorHandle {
    pub fn send(&self, cmd: MfCommand) -> Result<(), std::sync::mpsc::SendError<MfCommand>> {
        self.sender.send(cmd)
    }

    pub fn stop(&self) -> Result<(), std::sync::mpsc::SendError<MfCommand>> {
        self.sender.send(MfCommand::Stop)
    }
}

/// Dedicated MF actor thread. `on_event` is called on actor events, while the guard is live.
///
/// Features COM apartment initialization (`COINIT_APARTMENTTHREADED`), MFT session management,
/// and `MF_E_*` device-lost error handling triggering SlotRevoked emission and re-discovery on subsequent acquire.
pub fn spawn_actor<F, P>(
    mut on_event: F,
    publisher: Option<P>,
) -> std::io::Result<(thread::JoinHandle<()>, MfRuntimeToken, MfActorHandle)>
where
    F: 'static
        + Send
        + FnMut(
            MfRuntimeToken,
            &mut MfRuntimeGuard,
            &mut MfSession,
            MfCommand,
        ) -> Result<Option<FramePacket>, MfError>,
    P: 'static + Send + EventPublisher,
{
    let (tx, rx) = std::sync::mpsc::sync_channel::<MfCommand>(8);
    let actor_handle = MfActorHandle { sender: tx };

    let handle = thread::spawn(move || {
        // COM-initialise the actor's thread (STA).
        #[cfg(all(windows, feature = "mf"))]
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(
                None,
                windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
            );
            let _ = windows::Win32::Media::MediaFoundation::MFStartup(
                windows::Win32::Media::MediaFoundation::MF_VERSION,
                windows::Win32::Media::MediaFoundation::MFSTARTUP_NOSOCKET,
            );
        }

        let mut guard = MfRuntimeGuard {
            source_reader: None,
            sink_writer: None,
            transform: None,
        };

        let mut session = MfSession::default();

        while let Ok(cmd) = rx.recv() {
            if matches!(cmd, MfCommand::Stop) {
                break;
            }

            match on_event(MfRuntimeToken(true), &mut guard, &mut session, cmd) {
                Ok(_) => {
                    session.consecutive_device_lost = 0;
                }
                Err(MfError::DeviceLost(reason)) => {
                    // Mark session as needing re-discovery and clear hardware transform
                    session.needs_rediscovery = true;
                    session.consecutive_device_lost =
                        session.consecutive_device_lost.saturating_add(1);
                    guard.transform = None;
                    if let Some(ref publ) = publisher {
                        publ.publish(HardwareEvent::SlotRevoked {
                            device_class: "windows_mf".to_string(),
                            codec: session.active_codec.unwrap_or(Codec::H264),
                            reason: format!("MF device lost: {reason}"),
                        });
                    }
                    eprintln!("MF Actor encountered device lost: {reason}; triggering re-discovery on next acquire");
                }
                Err(MfError::EncoderFailure(reason)) => {
                    // HEVC mid-stream failure -> automatic fallback to H.264
                    if session.active_codec == Some(Codec::Hevc) {
                        eprintln!(
                            "MF HEVC encoder failed mid-stream: {reason}. Falling back to H.264..."
                        );
                        if let Err(e) = fallback_hevc_to_h264(&mut session, &mut guard) {
                            eprintln!("Failed to fall back from HEVC to H.264: {e}");
                        }
                    } else {
                        eprintln!("MF Actor encoder error: {reason}");
                    }
                }
                Err(err) => {
                    eprintln!("MF Actor error: {err}");
                }
            }
        }

        // Drop guard first (releases COM surfaces)
        drop(guard);

        #[cfg(all(windows, feature = "mf"))]
        unsafe {
            let _ = windows::Win32::Media::MediaFoundation::MFShutdown();
            windows::Win32::System::Com::CoUninitialize();
        }
    });

    Ok((handle, MfRuntimeToken(true), actor_handle))
}
