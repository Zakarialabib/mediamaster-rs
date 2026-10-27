//! Integration tests that exercise the **full adapter pattern** across all
//! four mediamaster-rs crates, using the SimpleSignage scenario as a case study.
//!
//! The mediamaster ecosystem is designed so that a downstream app (like
//! SignageMaster / SimpleSignage) implements core traits (`AudioSource`,
//! `SignalDetector`, `AsyncSensor`) and bridges hardware crates (`tv-rs`,
//! `cctv-rs`) to the mediamaster types.  These tests verify that the traits,
//! types, and event bus are cross-crate compatible.
