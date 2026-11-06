//! Platform abstraction for mirroring and screen sharing.

#[cfg(windows)]
pub mod win;
#[cfg(not(windows))]
pub mod win;

#[cfg(any(target_os = "linux", target_os = "android"))]
pub mod linux;
#[cfg(not(any(target_os = "linux", target_os = "android")))]
pub mod linux;

/// Configuration options for mirroring request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirrorConfig {
    pub session_id: u64,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub enable_audio: bool,
}

/// Keyframe request metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyframeRequest {
    pub session_id: u64,
    pub stream_id: u32,
}

/// User input events forwarded during mirroring (e.g. touch or mouse).
#[derive(Debug, Clone, PartialEq)]
pub enum MirrorInputEvent {
    PointerMove { x: f32, y: f32 },
    PointerDown { button: u32, x: f32, y: f32 },
    PointerUp { button: u32, x: f32, y: f32 },
    KeyDown { key_code: u32 },
    KeyUp { key_code: u32 },
}

/// Sink packet delivery during mirroring.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirrorSinkPacket<'a> {
    pub session_id: u64,
    pub is_video: bool,
    pub is_keyframe: bool,
    pub pts_us: u64,
    pub data: &'a [u8],
}

/// Abstract platform driver handling OS-specific capture, hardware display,
/// and input dispatch.
pub trait Platform: Send + Sync {
    /// Request mirroring start with configuration.
    fn mirror_requested(&self, config: &MirrorConfig) -> Result<(), i32>;

    /// Request mirror termination for a given session.
    fn mirror_stop_requested(&self, session_id: u64) -> Result<(), i32>;

    /// Request immediate keyframe generation.
    fn mirror_keyframe_requested(&self, request: &KeyframeRequest) -> Result<(), i32>;

    /// Inject input event into active mirror session.
    fn mirror_input(&self, session_id: u64, event: &MirrorInputEvent) -> Result<(), i32>;

    /// Deliver encoded/processed media packet to the platform sink.
    fn mirror_sink(&self, packet: &MirrorSinkPacket<'_>) -> Result<usize, i32>;
}

/// Returns the default platform instance for the current target OS.
pub fn default_platform() -> Box<dyn Platform> {
    #[cfg(windows)]
    {
        Box::new(win::WindowsPlatform::new())
    }
    #[cfg(not(windows))]
    {
        Box::new(linux::LinuxPlatform::new())
    }
}
