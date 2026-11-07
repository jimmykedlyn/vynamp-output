//! Test-only helpers shared by the vynamp-output crates.
//!
//! The core may use this crate only as a dev-dependency (zero-allocation tests),
//! never as a normal dependency; this crate depends on the core normally.

pub mod alloc;
pub mod source;
