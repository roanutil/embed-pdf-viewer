//! Binary payloads inside engine JSON: `{"$bytes": "<base64>"}`.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bytes(pub Vec<u8>);

impl Bytes {
    pub fn to_json(&self) -> Value {
        serde_json::json!({ "$bytes": STANDARD.encode(&self.0) })
    }

    pub fn from_json(value: &Value) -> Option<Bytes> {
        let text = value.get("$bytes")?.as_str()?;
        STANDARD.decode(text).ok().map(Bytes)
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
