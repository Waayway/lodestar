//! Helpers shared by integration tests (each test crate uses only part of them).
#![allow(dead_code, unused_imports)]

mod agent_auth;
mod fake_builder;
mod fake_issuer;
mod origin;

pub use agent_auth::{AGENT_TOKEN, agent_token_files, require_agent_token};
pub use fake_builder::{FakeBuilder, path_of};
pub use fake_issuer::FakeIssuer;
pub use origin::{Origin, git};
