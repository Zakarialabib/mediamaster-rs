#![allow(clippy::not_unsafe_ptr_arg_deref)]
//! mediamaster-ffi: Thin, runtime-free C ABI FFI layer bridging mediamaster-rs
//! to Tauri desktop player and Android mobile companion.
//!
//! # Architecture & Boundary Laws
//! - Only translates events across the FFI seam;
//! - Zero async runtime (Tokio is never linked, referenced, or run here);
//! - Speaks pure C ABI: raw structs, pointers, primitives, and C function pointer callbacks;
//! - All internal Rust types stay in the Rust workspace;
//! - Platform implementations (`win`, `linux`) reside behind `crate::platform`.

pub mod platform;

extern crate alloc;

use std::ffi::c_char;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

use platform::{
    default_platform, KeyframeRequest, MirrorConfig, MirrorInputEvent, MirrorSinkPacket, Platform,
};

/// Return status codes for FFI calls.
#[repr(i32)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum FfiResult {
    Ok = 0,
    InvalidArg = -1,
    SessionNotFound = -2,
    AlreadyStarted = -3,
    BufferTooSmall = -4,
    PlatformError = -5,
}

/// C ABI representation of RoomState.
#[repr(i32)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum FfiRoomState {
    Empty = 0,
    Occupied = 1,
    LowLight = 2,
    MotionDetected = 3,
    PersonPresent = 4,
    EmergencyOverride = 5,
    Unavailable = 6,
}

impl From<mediamaster_core::types::RoomState> for FfiRoomState {
    fn from(s: mediamaster_core::types::RoomState) -> Self {
        match s {
            mediamaster_core::types::RoomState::Empty => Self::Empty,
            mediamaster_core::types::RoomState::Occupied => Self::Occupied,
            mediamaster_core::types::RoomState::LowLight => Self::LowLight,
            mediamaster_core::types::RoomState::MotionDetected => Self::MotionDetected,
            mediamaster_core::types::RoomState::PersonPresent => Self::PersonPresent,
            mediamaster_core::types::RoomState::EmergencyOverride => Self::EmergencyOverride,
            mediamaster_core::types::RoomState::Unavailable => Self::Unavailable,
        }
    }
}

impl From<FfiRoomState> for mediamaster_core::types::RoomState {
    fn from(s: FfiRoomState) -> Self {
        match s {
            FfiRoomState::Empty => mediamaster_core::types::RoomState::Empty,
            FfiRoomState::Occupied => mediamaster_core::types::RoomState::Occupied,
            FfiRoomState::LowLight => mediamaster_core::types::RoomState::LowLight,
            FfiRoomState::MotionDetected => mediamaster_core::types::RoomState::MotionDetected,
            FfiRoomState::PersonPresent => mediamaster_core::types::RoomState::PersonPresent,
            FfiRoomState::EmergencyOverride => {
                mediamaster_core::types::RoomState::EmergencyOverride
            }
            FfiRoomState::Unavailable => mediamaster_core::types::RoomState::Unavailable,
        }
    }
}

/// C ABI event tag.
#[repr(i32)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum FfiEventKind {
    AudioDuckRequest = 1,
    SensorContextChange = 2,
    RadioBurstDetected = 3,
    CctvMotionDetected = 4,
    ZoneChanged = 5,
    EnvironmentalDrift = 6,
    AudioUnderrun = 7,
    AudioOverrun = 8,
    SlotRevoked = 9,
}

/// Flat C ABI event container with no heap pointers, safe to transfer across FFI.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct FfiHardwareEvent {
    pub kind: FfiEventKind,
    pub room_state: FfiRoomState,
    pub ducking_ratio: f32,
    pub ducking_duration_ms: u64,
    pub rf_frequency_hz: u32,
    pub rf_power_db: f32,
    pub motion_density: f32,
    pub ambient_lux: f32,
    pub audio_dropped_frames: u64,
    pub reserved: u64,
}

impl Default for FfiHardwareEvent {
    fn default() -> Self {
        Self {
            kind: FfiEventKind::SensorContextChange,
            room_state: FfiRoomState::Empty,
            ducking_ratio: 1.0,
            ducking_duration_ms: 0,
            rf_frequency_hz: 0,
            rf_power_db: 0.0,
            motion_density: 0.0,
            ambient_lux: 0.0,
            audio_dropped_frames: 0,
            reserved: 0,
        }
    }
}

/// Convert a mediamaster_core HardwareEvent into a flat C ABI event.
pub fn translate_core_event(event: &mediamaster_core::events::HardwareEvent) -> FfiHardwareEvent {
    let mut out = FfiHardwareEvent::default();
    match event {
        mediamaster_core::events::HardwareEvent::AudioDuckRequest { ducking, until, .. } => {
            out.kind = FfiEventKind::AudioDuckRequest;
            out.ducking_ratio = *ducking;
            out.ducking_duration_ms = *until;
        }
        mediamaster_core::events::HardwareEvent::SensorContextChange { state } => {
            out.kind = FfiEventKind::SensorContextChange;
            out.room_state = FfiRoomState::from(*state);
        }
        mediamaster_core::events::HardwareEvent::RadioBurstDetected { burst } => {
            out.kind = FfiEventKind::RadioBurstDetected;
            out.rf_frequency_hz = burst.frequency_hz;
            out.rf_power_db = burst.power_db;
        }
        mediamaster_core::events::HardwareEvent::CctvMotionDetected { .. } => {
            out.kind = FfiEventKind::CctvMotionDetected;
        }
        mediamaster_core::events::HardwareEvent::ZoneChanged { volume_db, .. } => {
            out.kind = FfiEventKind::ZoneChanged;
            out.ducking_ratio = *volume_db;
        }
        mediamaster_core::events::HardwareEvent::EnvironmentalDrift {
            motion_density,
            ambient_lux,
            ..
        } => {
            out.kind = FfiEventKind::EnvironmentalDrift;
            out.motion_density = *motion_density;
            out.ambient_lux = *ambient_lux;
        }
        mediamaster_core::events::HardwareEvent::AudioUnderrun { dropped_frames, .. } => {
            out.kind = FfiEventKind::AudioUnderrun;
            out.audio_dropped_frames = *dropped_frames as u64;
        }
        mediamaster_core::events::HardwareEvent::AudioOverrun { dropped_frames, .. } => {
            out.kind = FfiEventKind::AudioOverrun;
            out.audio_dropped_frames = *dropped_frames as u64;
        }
        mediamaster_core::events::HardwareEvent::SlotRevoked { .. } => {
            out.kind = FfiEventKind::SlotRevoked;
        }
        _ => {}
    }
    out
}

/// Convert a flat C ABI event back into mediamaster_core HardwareEvent.
pub fn translate_to_core_event(
    ffi_event: &FfiHardwareEvent,
) -> Option<mediamaster_core::events::HardwareEvent> {
    match ffi_event.kind {
        FfiEventKind::AudioDuckRequest => {
            Some(mediamaster_core::events::HardwareEvent::AudioDuckRequest {
                source: "ffi".into(),
                ducking: ffi_event.ducking_ratio,
                until: ffi_event.ducking_duration_ms,
            })
        }
        FfiEventKind::SensorContextChange => Some(
            mediamaster_core::events::HardwareEvent::SensorContextChange {
                state: ffi_event.room_state.into(),
            },
        ),
        FfiEventKind::RadioBurstDetected => Some(
            mediamaster_core::events::HardwareEvent::RadioBurstDetected {
                burst: mediamaster_core::types::RadioBurst {
                    id: "ffi-burst".into(),
                    frequency_hz: ffi_event.rf_frequency_hz,
                    power_db: ffi_event.rf_power_db,
                    payload: alloc::vec::Vec::new(),
                    detected_by: "ffi".into(),
                },
            },
        ),
        FfiEventKind::EnvironmentalDrift => Some(
            mediamaster_core::events::HardwareEvent::EnvironmentalDrift {
                motion_density: ffi_event.motion_density,
                mmwave_distance_m: 0.0,
                ambient_lux: ffi_event.ambient_lux,
                lux_rate_of_change: 0.0,
                rf_burst_frequency: 0.0,
            },
        ),
        FfiEventKind::AudioUnderrun => {
            Some(mediamaster_core::events::HardwareEvent::AudioUnderrun {
                source: "ffi".into(),
                dropped_frames: ffi_event.audio_dropped_frames as usize,
            })
        }
        FfiEventKind::AudioOverrun => Some(mediamaster_core::events::HardwareEvent::AudioOverrun {
            source: "ffi".into(),
            dropped_frames: ffi_event.audio_dropped_frames as usize,
        }),
        FfiEventKind::SlotRevoked => Some(mediamaster_core::events::HardwareEvent::SlotRevoked {
            device_class: "ffi".into(),
            codec: mediamaster_core::caps::Codec::H264,
            reason: "revoked over ffi".into(),
        }),
        _ => None,
    }
}

/// C ABI Mirror Configuration.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct FfiMirrorConfig {
    pub session_id: u64,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub enable_audio: u8,
}

/// C ABI Mirror Input Event Type.
#[repr(i32)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum FfiInputKind {
    PointerMove = 1,
    PointerDown = 2,
    PointerUp = 3,
    KeyDown = 4,
    KeyUp = 5,
}

/// C ABI Input Event.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct FfiInputEvent {
    pub kind: FfiInputKind,
    pub x: f32,
    pub y: f32,
    pub button: u32,
    pub key_code: u32,
}

/// Callback for event emission to host.
pub type FfiEventCallback =
    extern "C" fn(event: *const FfiHardwareEvent, user_data: *mut std::ffi::c_void);

struct GlobalState {
    platform: Box<dyn Platform>,
    callback: Option<FfiEventCallback>,
    user_data: usize,
    next_session_id: AtomicU64,
}

static STATE: RwLock<Option<GlobalState>> = RwLock::new(None);

fn with_state<F, R>(f: F) -> Result<R, i32>
where
    F: FnOnce(&GlobalState) -> Result<R, i32>,
{
    let guard = STATE.read().map_err(|_| FfiResult::PlatformError as i32)?;
    let state = guard.as_ref().ok_or(FfiResult::PlatformError as i32)?;
    f(state)
}

fn with_state_mut<F, R>(f: F) -> Result<R, i32>
where
    F: FnOnce(&mut GlobalState) -> Result<R, i32>,
{
    let mut guard = STATE.write().map_err(|_| FfiResult::PlatformError as i32)?;
    let state = guard.as_mut().ok_or(FfiResult::PlatformError as i32)?;
    f(state)
}

// ----------------------------------------------------------------------------
// Exported C ABI Functions
// ----------------------------------------------------------------------------

/// Initialize the mediamaster-ffi engine with the platform driver.
#[no_mangle]
pub extern "C" fn mediamaster_ffi_init() -> i32 {
    let mut guard = match STATE.write() {
        Ok(g) => g,
        Err(_) => return FfiResult::PlatformError as i32,
    };
    *guard = Some(GlobalState {
        platform: default_platform(),
        callback: None,
        user_data: 0,
        next_session_id: AtomicU64::new(1),
    });
    FfiResult::Ok as i32
}

/// Tear down mediamaster-ffi and release platform resources.
#[no_mangle]
pub extern "C" fn mediamaster_ffi_shutdown() -> i32 {
    let mut guard = match STATE.write() {
        Ok(g) => g,
        Err(_) => return FfiResult::PlatformError as i32,
    };
    *guard = None;
    FfiResult::Ok as i32
}

/// Register a native callback for hardware/room/media events.
#[no_mangle]
pub extern "C" fn mediamaster_ffi_set_event_callback(
    callback: Option<FfiEventCallback>,
    user_data: *mut std::ffi::c_void,
) -> i32 {
    with_state_mut(|state| {
        state.callback = callback;
        state.user_data = user_data as usize;
        Ok(FfiResult::Ok as i32)
    })
    .unwrap_or(FfiResult::PlatformError as i32)
}

/// Dispatch a hardware event into the bridge (translates and triggers callback).
#[no_mangle]
pub extern "C" fn mediamaster_ffi_dispatch_event(event: *const FfiHardwareEvent) -> i32 {
    if event.is_null() {
        return FfiResult::InvalidArg as i32;
    }
    let ev = unsafe { *event };

    with_state(|state| {
        if let Some(cb) = state.callback {
            cb(&ev, state.user_data as *mut std::ffi::c_void);
        }
        Ok(FfiResult::Ok as i32)
    })
    .unwrap_or(FfiResult::PlatformError as i32)
}

/// Request a new mirroring session. Writes assigned session_id if out_session_id is non-null.
#[no_mangle]
pub extern "C" fn mediamaster_ffi_mirror_requested(
    config: *const FfiMirrorConfig,
    out_session_id: *mut u64,
) -> i32 {
    if config.is_null() {
        return FfiResult::InvalidArg as i32;
    }
    let cfg = unsafe { *config };
    let session_id = if cfg.session_id == 0 {
        match with_state(|state| Ok(state.next_session_id.fetch_add(1, Ordering::SeqCst))) {
            Ok(id) => id,
            Err(e) => return e,
        }
    } else {
        cfg.session_id
    };

    let rust_config = MirrorConfig {
        session_id,
        width: cfg.width,
        height: cfg.height,
        fps: cfg.fps,
        enable_audio: cfg.enable_audio != 0,
    };

    let res = with_state(|state| state.platform.mirror_requested(&rust_config));

    match res {
        Ok(()) => {
            if !out_session_id.is_null() {
                unsafe {
                    *out_session_id = session_id;
                }
            }
            FfiResult::Ok as i32
        }
        Err(code) => code,
    }
}

/// Request mirror stop.
#[no_mangle]
pub extern "C" fn mediamaster_ffi_mirror_stop_requested(session_id: u64) -> i32 {
    let res = with_state(|state| state.platform.mirror_stop_requested(session_id));
    match res {
        Ok(()) => FfiResult::Ok as i32,
        Err(code) => code,
    }
}

/// Request immediate keyframe generation.
#[no_mangle]
pub extern "C" fn mediamaster_ffi_mirror_keyframe_requested(
    session_id: u64,
    stream_id: u32,
) -> i32 {
    let req = KeyframeRequest {
        session_id,
        stream_id,
    };
    let res = with_state(|state| state.platform.mirror_keyframe_requested(&req));
    match res {
        Ok(()) => FfiResult::Ok as i32,
        Err(code) => code,
    }
}

/// Forward input event to active mirror session.
#[no_mangle]
pub extern "C" fn mediamaster_ffi_mirror_input(
    session_id: u64,
    event: *const FfiInputEvent,
) -> i32 {
    if event.is_null() {
        return FfiResult::InvalidArg as i32;
    }
    let ev = unsafe { *event };
    let rust_event = match ev.kind {
        FfiInputKind::PointerMove => MirrorInputEvent::PointerMove { x: ev.x, y: ev.y },
        FfiInputKind::PointerDown => MirrorInputEvent::PointerDown {
            button: ev.button,
            x: ev.x,
            y: ev.y,
        },
        FfiInputKind::PointerUp => MirrorInputEvent::PointerUp {
            button: ev.button,
            x: ev.x,
            y: ev.y,
        },
        FfiInputKind::KeyDown => MirrorInputEvent::KeyDown {
            key_code: ev.key_code,
        },
        FfiInputKind::KeyUp => MirrorInputEvent::KeyUp {
            key_code: ev.key_code,
        },
    };

    let res = with_state(|state| state.platform.mirror_input(session_id, &rust_event));
    match res {
        Ok(()) => FfiResult::Ok as i32,
        Err(code) => code,
    }
}

/// Deliver a media packet to platform sink.
#[no_mangle]
pub extern "C" fn mediamaster_ffi_mirror_sink(
    session_id: u64,
    is_video: u8,
    is_keyframe: u8,
    pts_us: u64,
    data: *const u8,
    data_len: usize,
    out_written: *mut usize,
) -> i32 {
    if data.is_null() && data_len > 0 {
        return FfiResult::InvalidArg as i32;
    }
    let slice = if data_len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(data, data_len) }
    };

    let packet = MirrorSinkPacket {
        session_id,
        is_video: is_video != 0,
        is_keyframe: is_keyframe != 0,
        pts_us,
        data: slice,
    };

    let res = with_state(|state| state.platform.mirror_sink(&packet));
    match res {
        Ok(written) => {
            if !out_written.is_null() {
                unsafe {
                    *out_written = written;
                }
            }
            FfiResult::Ok as i32
        }
        Err(code) => code,
    }
}

/// Query version string (null-terminated C string).
#[no_mangle]
pub extern "C" fn mediamaster_ffi_version() -> *const c_char {
    c"0.3.0".as_ptr()
}
