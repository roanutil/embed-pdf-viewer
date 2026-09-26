//! `gate.ts`: the check between a signer's answer and `signatures.complete`.

use serde_json::{Value, json};

use crate::cms::{Cryptography, Profile, verify_parsed};
use crate::parse::parse_cms_internal;

/// `CompletionGate`.
#[derive(Clone, Debug, PartialEq)]
pub enum Gate {
    Ok,
    Refused { reason: &'static str, detail: String },
}

impl Gate {
    pub fn to_json(&self) -> Value {
        match self {
            Gate::Ok => json!({ "ok": true }),
            Gate::Refused { reason, detail } => {
                json!({ "ok": false, "reason": reason, "detail": detail })
            }
        }
    }
}

/// What the gate reads of `SignaturePrepared`.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    pub digest: Vec<u8>,
    /// As the engine spelled it; compared as text.
    pub algorithm: String,
    pub contents_size: f64,
}

/// `verifyForCompletion`.
pub fn verify_for_completion(cms: &[u8], prepared: &Prepared, profile: Profile) -> Gate {
    let refuse = |reason, detail: String| Gate::Refused { reason, detail };
    if cms.len() as f64 > prepared.contents_size {
        return refuse(
            "too-large",
            format!(
                "the CMS is {} bytes; {} were reserved",
                cms.len(),
                crate::number_text(prepared.contents_size)
            ),
        );
    }
    let internal = match parse_cms_internal(cms) {
        Ok(internal) => internal,
        Err(e) => return refuse("malformed", e.text()),
    };
    let parsed = &internal.parsed;
    if parsed.digest_algorithm.name() != prepared.algorithm {
        return refuse(
            "algorithm-mismatch",
            format!(
                "the CMS digests with {}, the document was prepared with {}",
                parsed.digest_algorithm.name(),
                prepared.algorithm
            ),
        );
    }
    if parsed.message_digest != prepared.digest {
        return refuse(
            "digest-mismatch",
            "the CMS message digest is not the prepared digest".into(),
        );
    }
    if profile == Profile::CadesB {
        if !parsed.signing_certificate_v2 {
            return refuse(
                "profile-violation",
                "CAdES requires the ESS signing-certificate-v2 attribute".into(),
            );
        }
        if parsed.signing_time.is_some() {
            return refuse(
                "profile-violation",
                "PAdES forbids the CMS signing-time attribute".into(),
            );
        }
    }
    match verify_parsed(&internal) {
        Cryptography::Valid => Gate::Ok,
        Cryptography::Unsupported => {
            refuse("signature-invalid", "the signature algorithm is not supported".into())
        }
        Cryptography::Invalid => {
            refuse("signature-invalid", "the signature does not verify".into())
        }
    }
}
