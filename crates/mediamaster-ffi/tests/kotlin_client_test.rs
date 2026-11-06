use std::process::Command;

#[test]
fn test_kotlin_client_drives_same_api_surface() {
    // Check prerequisites (kotlinc and java)
    let kotlinc_found = Command::new("kotlinc").arg("-version").output().is_ok();
    let jna_jar = std::path::Path::new("/usr/share/java/jna.jar");

    if !kotlinc_found || !jna_jar.exists() {
        eprintln!("Skipping Kotlin client test: kotlinc or jna.jar not found");
        return;
    }

    // 1. Ensure the shared library is compiled
    let status = Command::new("cargo")
        .args(["build", "-p", "mediamaster-ffi"])
        .status()
        .expect("cargo build failed");
    assert!(status.success());

    // 2. Locate target/debug/libmediamaster_ffi.{so,dll,dylib}
    let target_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target")
        .join("debug");
    let lib_name = format!(
        "{}mediamaster_ffi.{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_EXTENSION
    );
    let lib_path = target_dir.join(&lib_name);
    assert!(
        lib_path.exists(),
        "Shared library must exist at {:?}",
        lib_path
    );

    // 3. Create temp directory using std::env::temp_dir
    let temp_dir = std::env::temp_dir().join(format!("kt_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).expect("create temp dir");
    let kt_path = temp_dir.join("KotlinClientTest.kt");

    let kt_code = r#"
import com.sun.jna.*
import com.sun.jna.ptr.LongByReference
import java.io.File
import kotlin.system.exitProcess

interface MediamasterFfi : Library {
    @Structure.FieldOrder("kind", "room_state", "ducking_ratio", "ducking_duration_ms", "rf_frequency_hz", "rf_power_db", "motion_density", "ambient_lux", "audio_dropped_frames", "reserved")
    class FfiHardwareEvent : Structure() {
        @JvmField var kind: Int = 0
        @JvmField var room_state: Int = 0
        @JvmField var ducking_ratio: Float = 1.0f
        @JvmField var ducking_duration_ms: Long = 0
        @JvmField var rf_frequency_hz: Int = 0
        @JvmField var rf_power_db: Float = 0.0f
        @JvmField var motion_density: Float = 0.0f
        @JvmField var ambient_lux: Float = 0.0f
        @JvmField var audio_dropped_frames: Long = 0
        @JvmField var reserved: Long = 0
    }

    @Structure.FieldOrder("session_id", "width", "height", "fps", "enable_audio")
    class FfiMirrorConfig : Structure() {
        @JvmField var session_id: Long = 0
        @JvmField var width: Int = 0
        @JvmField var height: Int = 0
        @JvmField var fps: Int = 0
        @JvmField var enable_audio: Byte = 0
    }

    @Structure.FieldOrder("kind", "x", "y", "button", "key_code")
    class FfiInputEvent : Structure() {
        @JvmField var kind: Int = 0
        @JvmField var x: Float = 0.0f
        @JvmField var y: Float = 0.0f
        @JvmField var button: Int = 0
        @JvmField var key_code: Int = 0
    }

    interface EventCallback : Callback {
        fun invoke(event: FfiHardwareEvent?, userData: Pointer?)
    }

    fun mediamaster_ffi_init(): Int
    fun mediamaster_ffi_shutdown(): Int
    fun mediamaster_ffi_version(): String
    fun mediamaster_ffi_set_event_callback(callback: EventCallback?, userData: Pointer?): Int
    fun mediamaster_ffi_dispatch_event(event: FfiHardwareEvent): Int
    fun mediamaster_ffi_mirror_requested(config: FfiMirrorConfig, outSessionId: LongByReference?): Int
    fun mediamaster_ffi_mirror_stop_requested(sessionId: Long): Int
    fun mediamaster_ffi_mirror_keyframe_requested(sessionId: Long, streamId: Int): Int
    fun mediamaster_ffi_mirror_input(sessionId: Long, event: FfiInputEvent): Int
    fun mediamaster_ffi_mirror_sink(
        sessionId: Long,
        isVideo: Byte,
        isKeyframe: Byte,
        ptsUs: Long,
        data: ByteArray,
        dataLen: Long,
        outWritten: LongByReference?
    ): Int
}

fun main(args: Array<String>) {
    val libPath = args[0]
    val ffi = Native.load(libPath, MediamasterFfi::class.java) as MediamasterFfi

    // 1. Initialize
    check(ffi.mediamaster_ffi_init() == 0) { "init failed" }

    // 2. Version check
    val version = ffi.mediamaster_ffi_version()
    check(version == "0.3.0") { "Unexpected version: $version" }

    // 3. Register callback and test event dispatch
    var receivedKind = -1
    var receivedRoomState = -1
    val callback = object : MediamasterFfi.EventCallback {
        override fun invoke(event: MediamasterFfi.FfiHardwareEvent?, userData: Pointer?) {
            if (event != null) {
                receivedKind = event.kind
                receivedRoomState = event.room_state
            }
        }
    }
    check(ffi.mediamaster_ffi_set_event_callback(callback, null) == 0)

    val ev = MediamasterFfi.FfiHardwareEvent()
    ev.kind = 2 // SensorContextChange
    ev.room_state = 4 // PersonPresent
    check(ffi.mediamaster_ffi_dispatch_event(ev) == 0)
    check(receivedKind == 2) { "Callback expected kind 2, got $receivedKind" }
    check(receivedRoomState == 4) { "Callback expected room_state 4, got $receivedRoomState" }

    // 4. Request mirror session
    val cfg = MediamasterFfi.FfiMirrorConfig()
    cfg.session_id = 42L
    cfg.width = 1920
    cfg.height = 1080
    cfg.fps = 60
    cfg.enable_audio = 1
    val outSession = LongByReference()
    check(ffi.mediamaster_ffi_mirror_requested(cfg, outSession) == 0)
    check(outSession.value == 42L) { "Expected session 42, got ${outSession.value}" }

    // 5. Request keyframe
    check(ffi.mediamaster_ffi_mirror_keyframe_requested(42L, 1) == 0)

    // 6. Forward input
    val input = MediamasterFfi.FfiInputEvent()
    input.kind = 2 // PointerDown
    input.x = 100.5f
    input.y = 200.5f
    input.button = 1
    check(ffi.mediamaster_ffi_mirror_input(42L, input) == 0)

    // 7. Deliver packet to sink
    val sampleData = byteArrayOf(0xde.toByte(), 0xad.toByte(), 0xbe.toByte(), 0xef.toByte())
    val outWritten = LongByReference()
    check(ffi.mediamaster_ffi_mirror_sink(42L, 1, 1, 1000000L, sampleData, sampleData.size.toLong(), outWritten) == 0)
    check(outWritten.value == 4L) { "Expected 4 bytes written, got ${outWritten.value}" }

    // 8. Stop mirror session
    check(ffi.mediamaster_ffi_mirror_stop_requested(42L) == 0)

    // Operations after stop return SessionNotFound (-2)
    val resAfterStop = ffi.mediamaster_ffi_mirror_keyframe_requested(42L, 1)
    check(resAfterStop == -2) { "Expected SessionNotFound (-2), got $resAfterStop" }

    // 9. Shutdown
    check(ffi.mediamaster_ffi_shutdown() == 0)

    println("Kotlin client verified API surface successfully.")
}
"#;

    std::fs::write(&kt_path, kt_code).expect("write kt");

    // 4. Compile Kotlin test
    let jar_path = temp_dir.join("KotlinClientTest.jar");
    let compile_status = Command::new("kotlinc")
        .args([
            kt_path.to_str().unwrap(),
            "-cp",
            jna_jar.to_str().unwrap(),
            "-include-runtime",
            "-d",
            jar_path.to_str().unwrap(),
        ])
        .status()
        .expect("kotlinc failed");
    assert!(compile_status.success(), "Kotlin compilation failed");

    // 5. Run Kotlin client test with JNA linking libmediamaster_ffi.so
    let run_status = Command::new("java")
        .args([
            "-cp",
            &format!(
                "{}:{}",
                jar_path.to_str().unwrap(),
                jna_jar.to_str().unwrap()
            ),
            "KotlinClientTestKt",
            lib_path.to_str().unwrap(),
        ])
        .status()
        .expect("java run failed");
    assert!(run_status.success(), "Kotlin client test execution failed");

    let _ = std::fs::remove_dir_all(&temp_dir);
}
