//! Binary payloads inside engine JSON: `{"$bytes": "<base64>"}`, plus a
//! JavaScript-only escape hatch, `{"$hostBytes": n}`, for a payload the
//! sender chose to keep as a live value instead of inlining it (a rendered
//! image's pixels: see `packages/core-wasm/src/engine-bridge.ts`'s
//! `canonical`). `n` means nothing on its own; it resolves only through
//! [`set_host_bytes_resolver`].

use std::cell::RefCell;
use std::rc::Rc;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bytes(pub Vec<u8>);

thread_local! {
    /// What a `{"$hostBytes": n}` value resolves through. `None` until a
    /// host installs one (the wasm build's `WasmKernel::new`, over its
    /// engine bridge's `hostBytes(n)`); a kernel and its plugins live on
    /// one thread, so a thread_local is the one instance a build has.
    static HOST_BYTES_RESOLVER: RefCell<Option<Rc<dyn Fn(u32) -> Option<Vec<u8>>>>> =
        const { RefCell::new(None) };
}

/// Install (or, with `None`, clear) what `Bytes::from_json` calls for a
/// `{"$hostBytes": n}` value. With none installed, that shape resolves to
/// `None`, exactly like any other shape `from_json` does not recognize.
pub fn set_host_bytes_resolver(resolver: Option<Rc<dyn Fn(u32) -> Option<Vec<u8>>>>) {
    HOST_BYTES_RESOLVER.with(|cell| *cell.borrow_mut() = resolver);
}

impl Bytes {
    pub fn to_json(&self) -> Value {
        serde_json::json!({ "$bytes": STANDARD.encode(&self.0) })
    }

    pub fn from_json(value: &Value) -> Option<Bytes> {
        if let Some(text) = value.get("$bytes").and_then(Value::as_str) {
            return STANDARD.decode(text).ok().map(Bytes);
        }
        let n = value.get("$hostBytes").and_then(Value::as_u64)?;
        HOST_BYTES_RESOLVER
            .with(|cell| cell.borrow().as_ref().and_then(|resolve| resolve(n as u32)))
            .map(Bytes)
    }
}

impl Serialize for Bytes {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.to_json().serialize(s)
    }
}

impl<'de> Deserialize<'de> for Bytes {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(d)?;
        Bytes::from_json(&value).ok_or_else(|| D::Error::custom("expected {\"$bytes\": base64}"))
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use serde_json::json;

    use super::*;

    #[test]
    fn host_bytes_is_none_with_no_resolver_installed() {
        set_host_bytes_resolver(None);
        assert_eq!(Bytes::from_json(&json!({ "$hostBytes": 7 })), None);
    }

    #[test]
    fn host_bytes_resolves_through_an_installed_resolver() {
        set_host_bytes_resolver(Some(Rc::new(|n| (n == 7).then(|| vec![1, 2, 3]))));
        assert_eq!(Bytes::from_json(&json!({ "$hostBytes": 7 })), Some(Bytes(vec![1, 2, 3])));
        assert_eq!(Bytes::from_json(&json!({ "$hostBytes": 8 })), None);
        set_host_bytes_resolver(None);
    }
}
