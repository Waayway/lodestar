//! Helpers shared by integration tests (each test crate uses only part of them).
#![allow(dead_code)]

mod fake_issuer;

pub use fake_issuer::FakeIssuer;
