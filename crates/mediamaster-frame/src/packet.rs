//! Owned frame packet descriptors with zero-copy reference counting.

use std::sync::Arc;

use mediamaster_core::caps::{Codec, PixelFormat};
use mediamaster_core::frame::{FrameFlags, FrameRef};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Custom deallocator handle for pooled buffers.
pub(crate) trait PoolReclaim: Send + Sync {
    fn reclaim(&self, buf: Vec<u8>);
}

/// An owned buffer backing that returns its memory to a pool when dropped if pooled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FramePayload {
    /// Standard heap-allocated slice without pool tracking.
    ArcSlice(Arc<[u8]>),
    /// Reusable pooled buffer returning memory back to a `FramePool` upon final release.
    Pooled(Arc<PooledPayload>),
}

/// A reference-counted payload backed by a pooled Vec<u8>.
pub struct PooledPayload {
    pub(crate) data: Vec<u8>,
    pub(crate) reclaimer: Arc<dyn PoolReclaim>,
}

impl std::fmt::Debug for PooledPayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PooledPayload")
            .field("len", &self.data.len())
            .field("capacity", &self.data.capacity())
            .finish()
    }
}

impl PartialEq for PooledPayload {
    fn eq(&self, other: &Self) -> bool {
        self.data == other.data
    }
}

impl Eq for PooledPayload {}

impl Drop for PooledPayload {
    fn drop(&mut self) {
        let mem = std::mem::take(&mut self.data);
        self.reclaimer.reclaim(mem);
    }
}

impl core::ops::Deref for FramePayload {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &Self::Target {
        match self {
            FramePayload::ArcSlice(arc) => arc,
            FramePayload::Pooled(pooled) => &pooled.data,
        }
    }
}

#[cfg(feature = "serde")]
impl Serialize for FramePayload {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_bytes(self)
    }
}

#[cfg(feature = "serde")]
impl<'de> Deserialize<'de> for FramePayload {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let bytes = Vec::<u8>::deserialize(deserializer)?;
        Ok(FramePayload::ArcSlice(Arc::from(bytes.into_boxed_slice())))
    }
}

/// An owned frame packet containing metadata and a zero-copy payload.
///
/// Cloning a `FramePacket` is an $O(1)$ zero-copy operation that increments
/// the reference count of the payload buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct FramePacket {
    /// Zero-copy shared byte buffer for the frame data.
    pub payload: FramePayload,
    /// Presentation timestamp in microseconds.
    pub pts_us: i64,
    /// Decode timestamp in microseconds (optional).
    pub dts_us: Option<i64>,
    /// Codec format of the payload.
    pub codec: Codec,
    /// Pixel format or sample packing.
    pub pixel_format: PixelFormat,
    /// Coded or visible width in pixels.
    pub width: u16,
    /// Coded or visible height in pixels.
    pub height: u16,
    /// Row stride in bytes.
    pub stride: u16,
    /// Bitflags describing frame characteristics.
    pub flags: FrameFlags,
}

impl FramePacket {
    /// Create a new owned `FramePacket` from an `Arc<[u8]>` payload.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        payload: Arc<[u8]>,
        pts_us: i64,
        dts_us: Option<i64>,
        codec: Codec,
        pixel_format: PixelFormat,
        width: u16,
        height: u16,
        stride: u16,
        flags: FrameFlags,
    ) -> Self {
        Self {
            payload: FramePayload::ArcSlice(payload),
            pts_us,
            dts_us,
            codec,
            pixel_format,
            width,
            height,
            stride,
            flags,
        }
    }

    /// Create a new owned `FramePacket` with a generic [`FramePayload`].
    #[allow(clippy::too_many_arguments)]
    pub fn with_payload(
        payload: FramePayload,
        pts_us: i64,
        dts_us: Option<i64>,
        codec: Codec,
        pixel_format: PixelFormat,
        width: u16,
        height: u16,
        stride: u16,
        flags: FrameFlags,
    ) -> Self {
        Self {
            payload,
            pts_us,
            dts_us,
            codec,
            pixel_format,
            width,
            height,
            stride,
            flags,
        }
    }

    /// Promote a borrowed [`FrameRef`] into an owned `FramePacket` by copying its bytes.
    pub fn from_ref(frame_ref: FrameRef<'_>) -> Self {
        let payload: Arc<[u8]> = Arc::from(frame_ref.bytes);
        Self::new(
            payload,
            frame_ref.pts_us,
            frame_ref.dts_us,
            frame_ref.codec,
            frame_ref.pixel_format,
            frame_ref.width,
            frame_ref.height,
            frame_ref.stride,
            frame_ref.flags,
        )
    }

    /// Borrow this packet as a [`FrameRef`].
    pub fn as_ref(&self) -> FrameRef<'_> {
        FrameRef {
            bytes: &self.payload,
            pts_us: self.pts_us,
            dts_us: self.dts_us,
            codec: self.codec,
            pixel_format: self.pixel_format,
            width: self.width,
            height: self.height,
            stride: self.stride,
            flags: self.flags,
        }
    }

    /// Length in bytes of the payload buffer.
    #[inline]
    pub fn len(&self) -> usize {
        self.payload.len()
    }

    /// Check if the payload buffer is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.payload.is_empty()
    }

    /// Check if this frame is a keyframe.
    #[inline]
    pub fn is_key(&self) -> bool {
        self.flags.is_key()
    }

    /// Check if this frame is corrupt.
    #[inline]
    pub fn is_corrupt(&self) -> bool {
        self.flags.is_corrupt()
    }
}

impl<'a> From<FrameRef<'a>> for FramePacket {
    fn from(frame_ref: FrameRef<'a>) -> Self {
        Self::from_ref(frame_ref)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_packet_from_ref_and_as_ref() {
        let bytes = vec![1, 2, 3, 4, 5];
        let frame_ref = FrameRef {
            bytes: &bytes,
            pts_us: 42_000,
            dts_us: Some(40_000),
            codec: Codec::H264,
            pixel_format: PixelFormat::Nv12,
            width: 640,
            height: 480,
            stride: 640,
            flags: FrameFlags::KEY,
        };

        let packet = FramePacket::from(frame_ref);
        assert_eq!(packet.len(), 5);
        assert!(packet.is_key());
        assert!(!packet.is_corrupt());
        assert_eq!(packet.pts_us, 42_000);

        let roundtrip_ref = packet.as_ref();
        assert_eq!(roundtrip_ref.bytes, &bytes[..]);
        assert_eq!(roundtrip_ref.pts_us, 42_000);
        assert_eq!(roundtrip_ref.dts_us, Some(40_000));
        assert_eq!(roundtrip_ref.width, 640);
        assert_eq!(roundtrip_ref.flags, FrameFlags::KEY);
    }

    #[test]
    fn test_zero_copy_clone() {
        let data = vec![10u8; 1024];
        let packet = FramePacket::new(
            Arc::from(data.into_boxed_slice()),
            100,
            None,
            Codec::Hevc,
            PixelFormat::Yuv420p,
            1280,
            720,
            1280,
            FrameFlags::empty(),
        );

        let clone = packet.clone();
        assert_eq!(packet.payload.as_ptr(), clone.payload.as_ptr());
    }
}
