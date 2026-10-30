pub mod actions;
#[cfg(feature = "attest")]
pub mod attest;
pub mod challenge;
pub mod error;
pub mod frame;
pub mod session;
pub mod signals;
#[cfg(feature = "vision")]
pub mod vision;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
