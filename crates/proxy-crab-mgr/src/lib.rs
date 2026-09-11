//! Transport-neutral management API for ProxyCrab.

mod agents;
pub mod dto;
mod har;
pub mod har_share;
pub mod http;
pub mod manager;
pub mod permission;
pub mod session_share;
pub mod skill_install;

pub use manager::{MitmManager, ProxyCrabManager};
pub use proxy_crab_mitm::asset;
