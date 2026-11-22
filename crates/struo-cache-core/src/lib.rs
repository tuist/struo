//! Portable content identities shared by native and browser cache clients.

#![deny(missing_docs)]

mod action_key;
mod digest;

pub use action_key::ActionKeyBuilder;
pub use digest::Digest;

#[cfg(feature = "browser")]
mod browser;
