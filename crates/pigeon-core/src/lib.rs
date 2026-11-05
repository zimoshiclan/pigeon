//! Core of Pigeon: work out which post someone is looking at and turn it into
//! a clean link that's ready to send.

pub mod links;

#[cfg(windows)]
pub mod detect;

pub use links::{Kind, Link, Platform, parse};
