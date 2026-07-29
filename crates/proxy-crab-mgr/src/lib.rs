//! Transport-neutral management API for ProxyCrab.

pub mod dto;
pub mod http;
pub mod manager;

pub use manager::{MitmManager, ProxyCrabManager};
