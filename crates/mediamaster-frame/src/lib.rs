//! Frame-level packet descriptors, zero-copy `Arc` buffers, bounded rings, and fan-out
//! distribution for the `mediamaster-rs` ecosystem.
//!
//! # Architecture & Ownership Model
//!
//! In decoded media pipelines (e.g. video playback, camera preview, HLS segmentation,
//! thumbnail extraction), passing frames across subsystem boundaries without allocation
//! or data duplication is critical for low latency and high throughput.
//!
//! ## Who Frees What?
//!
//! - **[`mediamaster_core::FrameRef`] (Borrowed)**:
//!   `FrameRef` contains an immutable borrowed slice `&'a [u8]`. It owns no memory
//!   and performs no deallocation. It is intended for synchronous, zero-copy handoffs
//!   within the lifetime `'a` of a decoder's internal buffer or an active DMA mapping.
//!
//! - **[`FramePacket`] (Owned / Shared)**:
//!   `FramePacket` wraps metadata alongside an `Arc<[u8]>` (or pooled buffer representation).
//!   Cloning a `FramePacket` is an $O(1)$ zero-copy pointer and atomic reference count
//!   increment. The underlying buffer is deallocated when the final `Arc` reference drops.
//!
//! - **[`FramePool`] (Recycled Buffers)**:
//!   When high-resolution, uncompressed video frames (e.g. 4K NV12) are decoded at 60 fps,
//!   allocating and freeing multiple megabytes per frame triggers severe memory fragmentation
//!   and allocator contention. `FramePool` manages a bounded pool of pre-allocated buffers.
//!   Buffers checked out from `FramePool` are returned to the pool once the active
//!   consumers release their `PooledBuffer` or check it back in, keeping steady-state
//!   allocations bounded to the pool capacity after initial warmup.
//!
//! ## What Happens When a `FrameRef` Lifetime Ends Mid-Decode?
//!
//! If a producer yields a `FrameRef<'a>` that is tied to a transient decoder surface
//! (for example, a hardware decoder output buffer that will be overwritten or reclaimed
//! on the next decode step), consumers *must not* retain borrowed references beyond that step.
//! If an asynchronous consumer (such as a thumbnail generator or network segmenter) needs to
//! retain the frame across decode boundaries or thread hops, it **promotes** the `FrameRef`
//! into an owned [`FramePacket`] (via [`FramePacket::from_ref`] or using a buffer checked out
//! from [`FramePool`]). Promoting copies the slice exactly once into owned/pooled memory;
//! after that promotion, the original `FrameRef` may safely expire and the decoder surface
//! may be returned to the driver immediately.
//!
//! ## How `FramePool` Recycling Interacts With `FanOut` Clones
//!
//! When a producer fans out frames via [`FanOut`], each subscriber receives its own clone
//! of the [`FramePacket`]. All subscribers share the same underlying memory backing via
//! reference counting (`Arc`).
//! When `FramePool` is used:
//! 1. The producer acquires a buffer from `FramePool`.
//! 2. The producer decodes into or populates the buffer and builds a `FramePacket`.
//! 3. `FanOut` clones the packet across $N$ subscriber queues.
//! 4. Each subscriber consumes the frame at its own pace.
//! 5. As subscribers drop their respective `FramePacket` instances, the reference count
//!    decrements.
//! 6. When all subscribers (and the producer) have dropped their references, the buffer's
//!    reference count drops to 1 (or drops completely, depending on whether weak-ref tracking
//!    or explicit checkin is used), allowing the `FramePool` to safely reuse the buffer for a
//!    subsequent frame without reallocation.

pub mod fanout;
pub mod packet;
pub mod pool;
pub mod ring;

pub use fanout::{FanOut, FanOutSubscriber, SubscriberId};
pub use packet::FramePacket;
pub use pool::{FramePool, PooledBuffer};
pub use ring::{FrameRing, FrameRingConsumer, FrameRingProducer, OverflowPolicy};
