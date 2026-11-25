//! Vivy's engine: the agent loop, the phone connection, settings and chats.
//! Shared by the desktop app and the `vivy` command-line app; each front end
//! receives progress through an [`events::EventSink`].

pub mod agent;
pub mod events;
pub mod firewall;
pub mod log;
pub mod paths;
pub mod phone;
pub mod providers;
pub mod runner;
pub mod security;
pub mod sessions;
pub mod state;

/// The port the phone connects to.
pub const PHONE_PORT: u16 = 8765;
