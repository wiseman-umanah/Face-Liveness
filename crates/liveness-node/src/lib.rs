#![deny(clippy::all)]

use napi_derive::napi;

#[napi]
pub fn version() -> String {
  liveness_core::version().to_string()
}
