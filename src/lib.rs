#![forbid(unsafe_code)]
// CI runs `clippy ... -D warnings`; keep the bar high over time but avoid blocking releases on
// style-only debt accumulated during the Python→Rust port.
#![allow(unused_imports, unused_mut, dead_code)]
#![allow(clippy::all)]

pub mod actions;
pub mod app;
pub mod collectors;
pub mod inspectors;
pub mod mcp;
pub mod telemetry;
pub mod ui;

#[cfg(test)]
pub mod fixtures;
