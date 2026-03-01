/// Library crate entry point.
/// Exposes internal modules for integration tests.
/// Production binary uses src/main.rs.
pub mod api;
pub mod auth;
pub mod config;
pub mod db;
pub mod dns;
pub mod error;
pub mod metrics;
pub mod shutdown;
pub mod utils;
