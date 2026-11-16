//! The strict mode, in a test process of its own: its tests lock memory, and the long tests of
//! the suite use up the one limit a process has on locked memory while they run.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index fails the test, as it should"
)]

#[cfg(any(target_os = "linux", target_os = "android", target_vendor = "apple"))]
mod example;
#[cfg(any(target_os = "linux", target_os = "android", target_vendor = "apple"))]
mod strict;
#[cfg(not(any(target_os = "linux", target_os = "android", target_vendor = "apple")))]
mod strict_elsewhere;
