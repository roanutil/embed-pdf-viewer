//! The signature settings in the kernel's settings store.
//!
//! The kernel keeps a registration's settings as one JSON value
//! (`epdf_kernel::settings`). `mode` and `allowCertify` are plain data; the
//! `key` (a signer port or a function) and the `trust` port are not, so
//! each is stored as a handle, `{"$signatureKey": n}` or
//! `{"$signatureTrust": n}`, into a table the registration owns (one per
//! `signature_plugin(config)`, shared by its instances as the store is).
//! The same port reads back as the same value. Both are declared `whole`,
//! as `signature.plugin.ts` declares them: a change replaces them.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use epdf_signature_core::TrustPort;
use serde_json::{Map, Value, json};

use crate::contract::{SignatureConfig, SignatureKey, SignatureMode, SignatureSettings};

const KEY_HANDLE: &str = "$signatureKey";
const TRUST_HANDLE: &str = "$signatureTrust";

/// The registration's ports, by handle.
#[derive(Default)]
pub struct PortTable {
    keys: RefCell<Vec<(u64, SignatureKey)>>,
    trusts: RefCell<Vec<(u64, Rc<dyn TrustPort>)>>,
    next: Cell<u64>,
}

impl PortTable {
    fn mint(&self) -> u64 {
        let n = self.next.get();
        self.next.set(n + 1);
        n
    }

    fn key_json(&self, key: &Option<SignatureKey>) -> Value {
        let Some(key) = key else { return Value::Null };
        let found = self.keys.borrow().iter().find(|(_, k)| k.same(key)).map(|(n, _)| *n);
        let n = found.unwrap_or_else(|| {
            let n = self.mint();
            self.keys.borrow_mut().push((n, key.clone()));
            n
        });
        json!({ KEY_HANDLE: n })
    }

    fn trust_json(&self, trust: &Option<Rc<dyn TrustPort>>) -> Value {
        let Some(trust) = trust else { return Value::Null };
        let found = self
            .trusts
            .borrow()
            .iter()
            .find(|(_, t)| std::ptr::addr_eq(Rc::as_ptr(t), Rc::as_ptr(trust)))
            .map(|(n, _)| *n);
        let n = found.unwrap_or_else(|| {
            let n = self.mint();
            self.trusts.borrow_mut().push((n, trust.clone()));
            n
        });
        json!({ TRUST_HANDLE: n })
    }

    fn key_of(&self, value: &Value) -> Option<SignatureKey> {
        let n = value.get(KEY_HANDLE).and_then(Value::as_u64)?;
        self.keys.borrow().iter().find(|(m, _)| *m == n).map(|(_, k)| k.clone())
    }

    fn trust_of(&self, value: &Value) -> Option<Rc<dyn TrustPort>> {
        let n = value.get(TRUST_HANDLE).and_then(Value::as_u64)?;
        self.trusts.borrow().iter().find(|(m, _)| *m == n).map(|(_, t)| t.clone())
    }

    /// The JSON a `Partial<SignatureSettings>` change is in the store: only
    /// the members it names.
    pub fn encode(&self, config: &SignatureConfig) -> Value {
        let mut out = Map::new();
        if let Some(key) = &config.key {
            out.insert("key".into(), self.key_json(key));
        }
        if let Some(mode) = &config.mode {
            out.insert("mode".into(), mode.map_or(Value::Null, |m| json!(m.as_str())));
        }
        if let Some(trust) = &config.trust {
            out.insert("trust".into(), self.trust_json(trust));
        }
        if let Some(allow) = config.allow_certify {
            out.insert("allowCertify".into(), json!(allow));
        }
        Value::Object(out)
    }

    /// The settings a stored JSON value holds.
    pub fn decode(&self, value: &Value) -> SignatureSettings {
        SignatureSettings {
            key: value.get("key").and_then(|v| self.key_of(v)),
            mode: value.get("mode").and_then(Value::as_str).and_then(SignatureMode::from_name),
            trust: value.get("trust").and_then(|v| self.trust_of(v)),
            allow_certify: value.get("allowCertify").is_some_and(crate::js::truthy),
        }
    }
}

/// `SIGNATURE_DEFAULTS` in the store.
pub fn defaults_json() -> Value {
    json!({ "key": null, "mode": null, "trust": null, "allowCertify": false })
}

/// The registration's table and its registered settings.
pub fn register(config: &SignatureConfig) -> (Rc<PortTable>, Value) {
    let table = Rc::new(PortTable::default());
    let registered = table.encode(config);
    (table, registered)
}
