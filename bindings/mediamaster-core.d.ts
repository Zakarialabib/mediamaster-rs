// TypeScript definitions for mediamaster-core shared vocabulary.
// Generated and maintained for browser preview tooling and companion apps.

/**
 * A codec this ecosystem understands.
 */
export type Codec =
  | "H264"
  | "Hevc"
  | "Av1"
  | "Vp9"
  | "Mpeg2"
  | "Aac"
  | "Opus"
  | "Mp3"
  | "Flac";

/**
 * A pixel / sample format.
 */
export type PixelFormat = "Nv12" | "P010" | "Yuv420p" | "Rgb24";

/**
 * One codec's advertised capability on a host.
 */
export interface CodecCaps {
  codec: Codec;
  /** H.264/HEVC profile string (e.g. "High", "Main10"); empty for others. */
  profile: string;
  /** H.264/HEVC level (e.g. "5.1"); empty for others. */
  level: string;
  max_width: number;
  max_height: number;
  max_fps: number;
  /** Pixel formats this codec can be fed / produced in. */
  pixel_formats: PixelFormat[];
  /** true when backed by real hardware (not a software fallback). */
  hw_accelerated: bool_or_boolean;
}

type bool_or_boolean = boolean;

/**
 * How a layout degrades when the hardware cannot meet its demand.
 */
export type DegradationTier =
  | "Tier1HwSurface"
  | "Tier2HwByteBuffer"
  | "Tier3SharedTextureMux"
  | "Tier4PosterFallback";

/**
 * The full capability matrix for a host: every codec it advertises, plus the
 * hardware instance budget.
 */
export interface CapsMatrix {
  codecs: CodecCaps[];
  /**
   * Concurrent hardware decoder instances the SoC supports (Android MediaCodec
   * slot exhaustion guard). 0 = unknown.
   */
  max_hw_decoder_instances: number;
  /**
   * Concurrent hardware encoder instances. 0 = unknown.
   */
  max_hw_encoder_instances: number;
  /**
   * Confidence that the rows were verified on hardware (vs. only enumerated).
   */
  verified_on_hardware: boolean;
}

/**
 * An aggregate room state derived from fused sensor readings.
 */
export type RoomState =
  | "Empty"
  | "Occupied"
  | "LowLight"
  | "MotionDetected"
  | "PersonPresent"
  | "EmergencyOverride"
  | "Unavailable";

/**
 * A detected RF burst after protocol parsing.
 */
export interface RadioBurst {
  id: string;
  frequency_hz: number;
  /** Instantaneous power in dB relative to the detector's reference. */
  power_db: number;
  payload: number[];
  /** Which detector/parser produced this burst (e.g. "OOK@433MHz"). */
  detected_by: string;
}

/**
 * An axis-aligned rectangle, used for CCTV motion bounding boxes.
 */
export interface BoundingBox {
  x: number;
  y: number;
  width: number;
  height: number;
}

/**
 * A target for audio output on a playback device.
 */
export interface AudioTarget {
  label: string;
  device?: string | null;
  priority: number;
}

/**
 * One debounced sensor reading.
 */
export type SensorReading =
  | "MotionDetected"
  | { Proximity: number }
  | { Light: number }
  | "Occupancy"
  | "Open"
  | "Closed"
  | "Unknown";

/**
 * A detected RF signal — whatever a user's SignalDetector decides it is.
 */
export interface Signal {
  /** Human-readable identifier (e.g. bin index, protocol name). */
  id: string;
  /** Confidence score in [0.0, 1.0]. */
  confidence: number;
  /** Starting sample index in the original IQ stream. */
  start_sample: number;
  /** Peak magnitude observed. */
  magnitude_peak: number;
}

/**
 * Every event the ecosystem publishes and consumers subscribe to.
 */
export type HardwareEvent =
  | {
      AudioDuckRequest: {
        source: string;
        ducking: number;
        until: number;
      };
    }
  | {
      SensorContextChange: {
        state: RoomState;
      };
    }
  | {
      RadioBurstDetected: {
        burst: RadioBurst;
      };
    }
  | {
      CctvMotionDetected: {
        camera: string;
        bounding_box: BoundingBox;
      };
    }
  | {
      ZoneChanged: {
        zone_ids: string[];
        volume_db: number;
        muted: boolean;
      };
    }
  | {
      EnvironmentalDrift: {
        motion_density: number;
        mmwave_distance_m: number;
        ambient_lux: number;
        lux_rate_of_change: number;
        rf_burst_frequency: number;
      };
    };

/**
 * Stable identifier for a fleet node (desktop, edge, or Android player).
 */
export type NodeId = string;

/**
 * Which kind of host a node is.
 */
export type HostPlatform = "WindowsDesktop" | "LinuxEdge" | "AndroidMobile";

/**
 * A registered audio output endpoint reported by a node.
 */
export interface AudioTargetDescriptor {
  label: string;
  device?: string | null;
  priority: number;
}

/**
 * A node's probed media capability bundle.
 */
export interface MediaCapabilityBundleV1 {
  schema_version: string;
  node_id: NodeId;
  platform: HostPlatform;
  /** Concurrent hardware decoder instances (Android slot-exhaustion guard). */
  max_hw_decoder_instances: number;
  verified_decoders: CodecCaps[];
  verified_encoders: CodecCaps[];
  audio_outputs: AudioTargetDescriptor[];
  probed_at_unix_ms: number;
}

/**
 * An exclusive claim on a hardware codec slot (or a maintenance window).
 */
export interface MaintenanceLease {
  holder: NodeId;
  /** Monotonic millisecond expiry. */
  expires_at_ms: number;
  resource: string;
}
