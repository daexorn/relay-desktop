//! Shared Relay transport and data. The TUI can be built without a window system.
pub mod attachments;
pub mod cluster;
pub mod connection;
pub mod desktop;
pub mod gametest;
pub mod model;
pub mod storage;
pub mod templates;
pub mod uploads;

#[cfg(feature = "desktop")]
pub mod auth_browser;
#[cfg(feature = "desktop")]
pub mod images;
pub mod agent_icons;
