//! One module per target whose C toolchain comes from an SDK.
//!
//! The root of the crate keeps what the tool does regardless of the target: build one, run the
//! tests under coverage, and lay the reports out as the site.

pub mod android;
pub mod ios;
pub mod ohos;
pub mod wasm;
