//! `sign.ts`: prepare, sign, gate, complete, in one call.

use std::rc::Rc;

use epdf_kernel::Error;
use epdf_kernel::bytes::Bytes;
use epdf_kernel::engine::DocumentHandle;
use epdf_kernel::rt::Promise;
use serde_json::{Map, Value, json};

use crate::cms::{BuildInput, Profile, RawSigner, build_detached_cms};
use crate::gate::{Gate, Prepared, verify_for_completion};
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

/// `SignerPort`.
#[derive(Clone)]
pub enum SignerPort {
    Cms(CmsSigner),
    Raw(RawSigner),
}

/// `SigningError`.
pub fn signing_error(reason: &str, detail: &str) -> Error {
    Error::named("SigningError", format!("signing refused ({reason}): {detail}"))
        .with("reason", reason)
}

/// `profileFor`.
pub fn profile_for(sub_filter: &str) -> Profile {
    if sub_filter == "adbe.pkcs7.detached" { Profile::Pkcs7 } else { Profile::CadesB }
}

fn bytes_member(value: &Value, key: &str) -> Vec<u8> {
    value.get(key).and_then(Bytes::from_json).map(|b| b.0).unwrap_or_default()
}

/// `sign(doc, { ...prepareInput, signer })`. `input` is the prepare input
/// without the signer, as the TypeScript spreads it.
pub async fn sign(
    doc: Rc<dyn DocumentHandle>,
    input: Map<String, Value>,
    signer: SignerPort,
) -> Result<Value, Error> {
    if !doc.has("signatures") {
        return Err(Error::new("this engine does not implement signatures"));
    }
    let sub_filter = input
        .get("subFilter")
        .filter(|v| !epdf_kernel::is_undefined(v))
        .and_then(Value::as_str)
        .unwrap_or("ETSI.CAdES.detached")
        .to_string();
    let profile = profile_for(&sub_filter);
    let digest = match input.get("digest").filter(|v| !epdf_kernel::is_undefined(v)) {
        Some(d) => d.clone(),
        None => json!(match &signer {
            SignerPort::Raw(raw) => raw.hash.name(),
            SignerPort::Cms(_) => "sha256",
        }),
    };
    let mut request = input;
    request.insert("subFilter".into(), json!(sub_filter));
    request.insert("digest".into(), digest);
    let prepared = doc.call("signatures.prepare", None, vec![Value::Object(request)]).await?;
    let signing_id = prepared.get("signingId").cloned().unwrap_or(Value::Null);

    let attempt = async {
        let digest = bytes_member(&prepared, "digest");
        let algorithm =
            prepared.get("algorithm").and_then(Value::as_str).unwrap_or_default().to_string();
        let cms = match &signer {
            SignerPort::Cms(cms) => {
                (cms.sign)(CmsSignRequest {
                    digest: digest.clone(),
                    algorithm: algorithm.clone(),
                    sub_filter: sub_filter.clone(),
                })
                .await?
            }
            SignerPort::Raw(raw) => {
                let hash = Digest::from_name(&algorithm).ok_or_else(|| {
                    Error::new(format!(
                        "digest is {} bytes; {algorithm} needs undefined",
                        digest.len()
                    ))
                })?;
                build_detached_cms(BuildInput {
                    digest: digest.clone(),
                    hash,
                    profile,
                    signer: raw.clone(),
                    signing_time: (profile == Profile::Pkcs7).then(epdf_kernel::rt::now),
                })
                .await?
            }
        };
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
            return Err(signing_error(reason, &detail));
        }
        doc.call(
            "signatures.complete",
            None,
            vec![json!({
                "signingId": signing_id,
                "cms": Bytes(cms).to_json(),
                "expectedVersion": prepared.get("expectedVersion").cloned().unwrap_or(Value::Null),
            })],
        )
        .await
    };
    match attempt.await {
        Ok(result) => Ok(result),
        Err(error) => {
            let _ = doc.call("signatures.abort", None, vec![signing_id.clone()]).await;
            Err(error)
        }
    }
}
