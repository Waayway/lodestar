//! Helpers shared by integration tests (each test crate uses only part of them).
#![allow(dead_code, unused_imports)]

mod fake_issuer;
mod origin;

pub use fake_issuer::FakeIssuer;
pub use origin::{Origin, git};
