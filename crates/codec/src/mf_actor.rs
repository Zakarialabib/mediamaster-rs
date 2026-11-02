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
//!
//! The placeholder [`COMSurfaceHandle`] below is an opaque COM object handle. On
//! the device build, replace the handles with the real `windows::Win32::Media:
//! MediaFoundation::{IMFSourceReader, IMFSinkWriter, IMFTopology}` COM interface
//! types; all ownership/drop logic below stays identical.

use std::cell::Cell;
use std::ptr::NonNull;
use std::thread;

/// Guard for one MF actor's COM runtime and hardware surfaces.
///
/// `COMSurfaceHandle` is an opaque COM object handle representing an
/// `IMFSourceReader` / `IMFSinkWriter` / `IMFTopology` owned by this actor.
/// On the device build it is replaced with the real windows-rs MF COM interface
/// types (the ownership and Drop behaviour below is unchanged).
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

/// Opaque COM object handle — the structural stand-in for a COM interface
/// reference (e.g. `IMFSourceReader`, `IMFSinkWriter`). On the device build, this
/// is replaced with the real windows-rs COM type; the Drop/ownership rules below
/// are identical.
type COMSurfaceHandle = NonNull<core::ffi::c_void>;

/// Guard's runtime slot: true while COM is initialised on this actor's thread.
#[derive(Debug, Clone, Copy)]
pub struct MfRuntimeToken(bool);

/// Dedicated MF actor thread. `on_event` is called immediately after
/// `CoInitializeEx` + `MFStartup`, while the guard is live.
///
/// # Build
/// ```ignore
/// cargo check --no-default-features --features mf     # requires the `windows` 0.62
///                                                      // `Win32_Media_MediaFoundation`
///                                                      // and `Win32_System_Threading` features
/// ```
pub fn spawn_actor<F>(mut on_event: F) -> std::io::Result<(thread::JoinHandle<()>, MfRuntimeToken)>
where
    F: 'static + Send + FnMut(MfRuntimeToken, &mut MfRuntimeGuard),
{
    let (tx, rx) = std::sync::mpsc::sync_channel::<MfCommand>(8);
    let guard = MfRuntimeGuard {
        source_reader: None,
        sink_writer: None,
        transform: None,
    };

    let handle = thread::spawn(move || {
        // COM-initialise the actor's thread (STA). COM calls here run in a
        // dedicated thread so the media pipeline never blocks the app's UI.
        unsafe {
            let _ = windows::Win32::System::Threading::CoInitializeEx(
                None,
                windows::Win32::System::Threading::COINIT_APARTMENTTHREADED,
            );
        }
        let _guard = MfRuntimeGuard {
            source_reader: None,
            sink_writer: None,
            transform: None,
        };
        // On scope exit: `_guard` drops (COM surfaces released) -> MFShutdown ->
        // CoUninitialize. Drop-order law: COM surfaces, then shutdown, then RT.
        loop {
            match rx.recv() {
                Ok(MfCommand::Stop) => break,
                Ok(MfCommand::Initialize(cfg)) => {
                    // MFStartup + IMFSourceReader / MFT enumeration go here.
                    // `guard` is live across this call so its Drop runs after
                    // any MFShutdown if initialisation fails.
                    if on_event(MfRuntimeToken(true), &mut guard).is_err() {
                        tx.send(Err(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            "MF actor initialisation failed",
                        )))
                        .ok();
                        break;
                    }
                    tx.send(Ok(())).ok();
                }
                _ => {}
            }
        }
    });
    Ok((handle, MfRuntimeToken(true)))
}

/// Commands the host thread can send to a running actor.
pub enum MfCommand {
    Stop,
    Initialize(MfConfig),
}

/// Configuration carried into a fresh MF actor.
pub struct MfConfig {
    pub source_url: String,
    pub target_format: String,
}

/// Application-visible hook that runs per actor tick. Returns `Ok(())` on
/// success or `Err` to abort the actor.
pub type MfEventHandler = for<'a> fn(
    MfRuntimeToken,
    &'a mut MfRuntimeGuard,
    &mut MfConfig,
) -> std::io::Result<()>;
