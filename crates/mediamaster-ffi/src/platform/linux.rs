//! Linux/Android-specific platform implementation for mirroring.

use super::{KeyframeRequest, MirrorConfig, MirrorInputEvent, MirrorSinkPacket, Platform};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Linux/Android platform driver.
#[derive(Debug, Default)]
pub struct LinuxPlatform {
    active_session: AtomicU64,
    is_running: AtomicBool,
}

impl LinuxPlatform {
    pub const fn new() -> Self {
        Self {
            active_session: AtomicU64::new(0),
            is_running: AtomicBool::new(false),
        }
    }
}

impl Platform for LinuxPlatform {
    fn mirror_requested(&self, config: &MirrorConfig) -> Result<(), i32> {
        self.active_session
            .store(config.session_id, Ordering::SeqCst);
        self.is_running.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn mirror_stop_requested(&self, session_id: u64) -> Result<(), i32> {
        let current = self.active_session.load(Ordering::SeqCst);
        if current == session_id {
            self.is_running.store(false, Ordering::SeqCst);
            self.active_session.store(0, Ordering::SeqCst);
            Ok(())
        } else {
            Err(-2) // SessionNotFound
        }
    }

    fn mirror_keyframe_requested(&self, request: &KeyframeRequest) -> Result<(), i32> {
        let current = self.active_session.load(Ordering::SeqCst);
        if current == request.session_id && self.is_running.load(Ordering::SeqCst) {
            Ok(())
        } else {
            Err(-2)
        }
    }

    fn mirror_input(&self, session_id: u64, _event: &MirrorInputEvent) -> Result<(), i32> {
        let current = self.active_session.load(Ordering::SeqCst);
        if current == session_id && self.is_running.load(Ordering::SeqCst) {
            Ok(())
        } else {
            Err(-2)
        }
    }

    fn mirror_sink(&self, packet: &MirrorSinkPacket<'_>) -> Result<usize, i32> {
        let current = self.active_session.load(Ordering::SeqCst);
        if current == packet.session_id && self.is_running.load(Ordering::SeqCst) {
            Ok(packet.data.len())
        } else {
            Err(-2)
        }
    }
}
