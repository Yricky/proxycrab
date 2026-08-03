//! Transport-neutral management API for ProxyCrab.

mod agents;
pub mod dto;
pub mod http;
pub mod manager;

pub use manager::{MitmManager, ProxyCrabManager};
