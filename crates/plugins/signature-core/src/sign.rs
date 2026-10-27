//! `sign.ts`: prepare, sign, gate, complete, in one call.

use std::rc::Rc;

use epdf_kernel::Error;
use epdf_kernel::bytes::Bytes;
use epdf_kernel::engine::DocumentHandle;
use epdf_kernel::rt::Promise;
use serde_json::{Map, Value, json};

use crate::cms::{BuildInput, Profile, RawSigner, build_detached_cms};
use crate::gate::{CompletionProfile, Gate, Prepared, verify_for_completion};
use crate::oids::Digest;

/// What a `CmsSigner` is asked for.
#[derive(Clone, Debug, PartialEq)]
pub struct CmsSignRequest {
    pub digest: Vec<u8>,
    pub algorithm: String,
    pub sub_filter: String,
}

impl CmsSignRequest {
    pub fn to_json(&self) -> Value {
        json!({ "digest": Bytes(self.digest.clone()).to_json(), "algorithm": self.algorithm, "subFilter": self.sub_filter })
    }
}

/// `CmsSigner`: a service that turns a digest into a finished CMS.
#[derive(Clone)]
pub struct CmsSigner {
    pub sign: Rc<dyn Fn(CmsSignRequest) -> Promise<Vec<u8>>>,
}

/// `SignerPort`: the `key` of a `SignInput`.
#[derive(Clone)]
pub enum SignerPort {
    Cms(CmsSigner),
    Raw(RawSigner),
}

/// `signingRefused`: the gate refused the key's answer. An `EngineError`
/// with code `SignatureRefused` and the gate's reason in `details.reason`.
pub fn signing_refused(reason: &str, detail: &str) -> Error {
    Error::engine("SignatureRefused", format!("signing refused ({reason}): {detail}"))
        .with("details", json!({ "reason": reason }))
}

/// `profileFor`: the CMS shape a `/SubFilter` promises (`timestamp` for an
/// RFC 3161 token); `InvalidArg` for one this package doesn't know.
pub fn profile_for(sub_filter: &str) -> Result<CompletionProfile, Error> {
    match sub_filter {
        "adbe.pkcs7.detached" => Ok(CompletionProfile::Signature(Profile::Pkcs7)),
        "ETSI.CAdES.detached" => Ok(CompletionProfile::Signature(Profile::CadesB)),
        "ETSI.RFC3161" => Ok(CompletionProfile::Timestamp),
        other => Err(Error::engine("InvalidArg", format!("unknown subFilter '{other}'"))
            .with("details", json!({ "field": "subFilter" }))),
    }
}

fn bytes_member(value: &Value, key: &str) -> Vec<u8> {
    value.get(key).and_then(Bytes::from_json).map(|b| b.0).unwrap_or_default()
}

/// A member as JavaScript reads it: `undefined` when absent.
fn member(value: &Value, key: &str) -> Value {
    value.get(key).cloned().unwrap_or_else(epdf_kernel::undefined)
}

/// `value ?? fallback`: an absent, `undefined` or `null` member is nullish.
fn present<'a>(input: &'a Map<String, Value>, key: &str) -> Option<&'a Value> {
    input.get(key).filter(|v| !v.is_null() && !epdf_kernel::is_undefined(v))
}

/// `String(value)`.
fn js_string(value: &Value) -> String {
    match value {
        v if epdf_kernel::is_undefined(v) => "undefined".into(),
        Value::String(s) => s.clone(),
        Value::Number(n) => crate::number_text(n.as_f64().unwrap_or(0.0)),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".into(),
        // `Array.prototype.join`: null and undefined members print empty.
        Value::Array(items) => items
            .iter()
            .map(|v| {
                if v.is_null() || epdf_kernel::is_undefined(v) {
                    String::new()
                } else {
                    js_string(v)
                }
            })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".into(),
    }
}

/// `sign(doc, { ...prepareInput, key })`: `prepare` on the engine, the key
/// turns the digest into a CMS (or a raw signature this package wraps), the
/// gate checks the CMS matches what was prepared, `complete` installs it.
/// Any failure after `prepare` cancels the candidate and rethrows.
///
/// `input` is `SignInput` without its `key`, as the TypeScript spreads it:
/// the `SignaturePrepareInput` members (`field`, `kind`, `subFilter`,
/// `digest`, `contentsSize`, `signer`, `certify`, `lock`, `appearance`),
/// passed to `signatures.prepare` in the caller's order. For a document
/// timestamp (`kind: 'timestamp'`) the key is a timestamp authority: a CMS
/// key that returns an RFC 3161 token.
pub async fn sign(
    doc: Rc<dyn DocumentHandle>,
    input: Map<String, Value>,
    key: SignerPort,
) -> Result<Value, Error> {
    let timestamp = input.get("kind").and_then(Value::as_str) == Some("timestamp");
    if timestamp && !matches!(key, SignerPort::Cms(_)) {
        return Err(Error::engine(
            "InvalidArg",
            "a document timestamp needs a timestamp authority: a CMS key that returns an RFC 3161 token",
        )
        .with("details", json!({ "field": "key" })));
    }
    let sub_filter = if timestamp {
        Value::from("ETSI.RFC3161")
    } else {
        present(&input, "subFilter").cloned().unwrap_or_else(|| "ETSI.CAdES.detached".into())
    };
    // `profileFor` switches with strict equality: only a string can name a
    // sub-filter, whatever another value prints as.
    let profile = match &sub_filter {
        Value::String(name) => profile_for(name)?,
        other => {
            return Err(Error::engine(
                "InvalidArg",
                format!("unknown subFilter '{}'", js_string(other)),
            )
            .with("details", json!({ "field": "subFilter" })));
        }
    };
    let sub_filter = js_string(&sub_filter);
    let digest = match present(&input, "digest") {
        Some(d) => d.clone(),
        None => json!(match &key {
            SignerPort::Raw(raw) => raw.hash.name(),
            SignerPort::Cms(_) => "sha256",
        }),
    };
    let mut request = input;
    if !timestamp {
        request.insert("subFilter".into(), json!(sub_filter));
    }
    request.insert("digest".into(), digest);
    let prepared = doc.call("signatures.prepare", None, vec![Value::Object(request)]).await?;
    let signing_id = member(&prepared, "signingId");

    let attempt = async {
        let cms = cms_for(&key, &prepared, &sub_filter, profile).await?;
        let digest = bytes_member(&prepared, "digest");
        let algorithm =
            prepared.get("algorithm").and_then(Value::as_str).unwrap_or_default().to_string();
        let gate = verify_for_completion(
            &cms,
            &Prepared {
                digest,
                algorithm,
                contents_size: prepared
                    .get("contentsSize")
                    .and_then(Value::as_f64)
                    .unwrap_or(f64::NAN),
            },
            profile,
        );
        if let Gate::Refused { reason, detail } = gate {
            return Err(signing_refused(reason, &detail));
        }
        doc.call(
            "signatures.complete",
            None,
            vec![json!({
                "signingId": signing_id,
                "cms": Bytes(cms).to_json(),
                "expectedVersion": member(&prepared, "expectedVersion"),
            })],
        )
        .await
    };
    match attempt.await {
        Ok(result) => Ok(result),
        Err(error) => {
            let _ = doc.call("signatures.cancel", None, vec![signing_id.clone()]).await;
            Err(error)
        }
    }
}

/// `cmsFor`: the key's answer for the prepared digest, a service's CMS, or
/// one built here around a raw signature.
async fn cms_for(
    key: &SignerPort,
    prepared: &Value,
    sub_filter: &str,
    profile: CompletionProfile,
) -> Result<Vec<u8>, Error> {
    let digest = bytes_member(prepared, "digest");
    let algorithm = js_string(&member(prepared, "algorithm"));
    match key {
        SignerPort::Cms(cms) => {
            (cms.sign)(CmsSignRequest { digest, algorithm, sub_filter: sub_filter.to_string() })
                .await
        }
        SignerPort::Raw(raw) => {
            let CompletionProfile::Signature(profile) = profile else {
                return Err(Error::engine("InvalidArg", "a raw key cannot make a timestamp token"));
            };
            let hash = Digest::from_name(&algorithm).ok_or_else(|| {
                Error::new(format!("digest is {} bytes; {algorithm} needs undefined", digest.len()))
            })?;
            build_detached_cms(BuildInput {
                digest,
                hash,
                profile,
                signer: raw.clone(),
                signing_time: (profile == Profile::Pkcs7).then(epdf_kernel::rt::now),
            })
            .await
        }
    }
}
