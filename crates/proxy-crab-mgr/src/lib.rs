//! Transport-neutral management API for ProxyCrab.

mod agents;
pub mod dto;
mod har;
pub mod http;
pub mod manager;
pub mod permission;

pub use manager::{MitmManager, ProxyCrabManager};
