//! Apple and Android binding layer.
//!
//! Holds `uniffi::setup_scaffolding!()` and a newtype plus a `From` impl per
//! exported type. It stays a separate crate so the scaffolding macro never has
//! to build for `wasm32-unknown-emscripten`; the cost is that newtype-and-`From`
//! per type.
//!
//! Consumes `epdf-engine` and adds no semantics of its own.
