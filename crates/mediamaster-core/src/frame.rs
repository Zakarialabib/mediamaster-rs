//! Frame-level packet descriptors for decoded media samples.
//!
//! [`FrameRef`] is a borrowed descriptor over raw sample bytes, designed to be
//! passed across crate boundaries in `no_std` environments without copying.

#[cfg(feature = "serde-sync")]
use serde::{Deserialize, Serialize};

use crate::caps::{Codec, PixelFormat};

/// Bitflags representing frame metadata and delivery state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde-sync", serde(transparent))]
pub struct FrameFlags(pub u32);

impl FrameFlags {
    /// Empty flags (no special attributes set).
    pub const NONE: Self = Self(0);
    /// Keyframe / random access point (IDR / I-frame).
    pub const KEY: Self = Self(1 << 0);
    /// Intra-coded frame (intra prediction only).
    pub const INTRA: Self = Self(1 << 1);
    /// Corrupted sample / transmission error indicated by decoder.
    pub const CORRUPT: Self = Self(1 << 2);
    /// Frame was marked as dropped or non-reference discarded.
    pub const DROPPED: Self = Self(1 << 3);

    /// Create empty flags.
    #[inline]
    pub const fn empty() -> Self {
        Self::NONE
    }

    /// Create from raw u32 bits.
    #[inline]
    pub const fn from_bits_retain(bits: u32) -> Self {
        Self(bits)
    }

    /// Get the underlying bits.
    #[inline]
    pub const fn bits(&self) -> u32 {
        self.0
    }

    /// Check if all flags in `other` are set in `self`.
    #[inline]
    pub const fn contains(&self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    /// Check if this frame is a keyframe.
    #[inline]
    pub const fn is_key(&self) -> bool {
        self.contains(Self::KEY)
    }

    /// Check if this frame is intra-coded.
    #[inline]
    pub const fn is_intra(&self) -> bool {
        self.contains(Self::INTRA)
    }

    /// Check if this frame has corruption flags.
    #[inline]
    pub const fn is_corrupt(&self) -> bool {
        self.contains(Self::CORRUPT)
    }

    /// Check if this frame was marked as dropped.
    #[inline]
    pub const fn is_dropped(&self) -> bool {
        self.contains(Self::DROPPED)
    }

    /// Set flags.
    #[inline]
    pub const fn insert(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Remove flags.
    #[inline]
    pub const fn remove(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }
}

impl core::ops::BitOr for FrameFlags {
    type Output = Self;

    #[inline]
    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl core::ops::BitOrAssign for FrameFlags {
    #[inline]
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl core::ops::BitAnd for FrameFlags {
    type Output = Self;

    #[inline]
    fn bitand(self, rhs: Self) -> Self::Output {
        Self(self.0 & rhs.0)
    }
}

impl core::ops::BitAndAssign for FrameFlags {
    #[inline]
    fn bitand_assign(&mut self, rhs: Self) {
        self.0 &= rhs.0;
    }
}

impl core::ops::Not for FrameFlags {
    type Output = Self;

    #[inline]
    fn not(self) -> Self::Output {
        Self(!self.0)
    }
}

/// A borrowed frame descriptor referencing an in-memory sample buffer.
///
/// Designed to be passed across crate boundaries in `no_std` environments without copying.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
pub struct FrameRef<'a> {
    /// Borrowed sample slice (e.g. decoded NV12, RGBA, or raw bitstream NALs).
    pub bytes: &'a [u8],
    /// Presentation timestamp in microseconds.
    pub pts_us: i64,
    /// Decode timestamp in microseconds (optional; e.g. for B-frame reordering).
    pub dts_us: Option<i64>,
    /// Codec format of the payload.
    pub codec: Codec,
    /// Pixel format or sample packing.
    pub pixel_format: PixelFormat,
    /// Coded or visible width in pixels.
    pub width: u16,
    /// Coded or visible height in pixels.
    pub height: u16,
    /// Row stride in bytes (bytes per row in the primary plane).
    pub stride: u16,
    /// Bitflags describing frame characteristics (key, corrupt, dropped, etc.).
    pub flags: FrameFlags,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flags() {
        let f = FrameFlags::KEY | FrameFlags::INTRA;
        assert!(f.is_key());
        assert!(f.is_intra());
        assert!(!f.is_corrupt());
        assert!(!f.is_dropped());

        let corrupt = f | FrameFlags::CORRUPT;
        assert!(corrupt.is_corrupt());

        let cleared = corrupt.remove(FrameFlags::KEY);
        assert!(!cleared.is_key());
        assert!(cleared.is_intra());
    }

    #[test]
    fn test_frame_ref_creation() {
        let dummy = [0u8; 16];
        let frame = FrameRef {
            bytes: &dummy,
            pts_us: 1000,
            dts_us: Some(950),
            codec: Codec::H264,
            pixel_format: PixelFormat::Nv12,
            width: 1920,
            height: 1080,
            stride: 1920,
            flags: FrameFlags::KEY,
        };

        assert_eq!(frame.width, 1920);
        assert_eq!(frame.height, 1080);
        assert!(frame.flags.is_key());
        assert_eq!(frame.bytes.len(), 16);
    }
}
