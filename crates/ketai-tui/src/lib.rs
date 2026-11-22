//! Cliente de terminal de ketai. Habla con `ketaid` por el protocolo IPC
//! (docs/protocol.md); nunca reproduce audio por su cuenta.

pub mod app;
pub mod client;
pub mod config;
pub mod covers;
pub mod kitty;
pub mod service;
pub mod theme;
pub mod ui;
pub mod viz;
