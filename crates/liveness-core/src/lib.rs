pub mod actions;
pub mod challenge;
pub mod error;
pub mod frame;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
