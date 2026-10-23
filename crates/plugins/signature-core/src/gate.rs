//! `gate.ts`: the check between a signer's answer and `signatures.complete`.

use serde_json::{Value, json};

use crate::cms::{Cryptography, Profile, verify_parsed};
use crate::parse::{CmsContent, parse_cms_content, parse_cms_internal};
use crate::timestamp::read_timestamp_info;

/// `CompletionProfile`: the shape the CMS must have, a PDF signature's
/// (`pkcs7`, `cades-b`), or a document timestamp's RFC 3161 token
/// (`timestamp`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompletionProfile {
    Signature(Profile),
    Timestamp,
}

impl CompletionProfile {
    pub fn from_name(name: &str) -> Option<CompletionProfile> {
        match name {
            "timestamp" => Some(CompletionProfile::Timestamp),
            other => Profile::from_name(other).map(CompletionProfile::Signature),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            CompletionProfile::Signature(profile) => profile.name(),
            CompletionProfile::Timestamp => "timestamp",
        }
    }
}

impl From<Profile> for CompletionProfile {
    fn from(profile: Profile) -> Self {
        CompletionProfile::Signature(profile)
    }
}

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
pub fn verify_for_completion(
    cms: &[u8],
    prepared: &Prepared,
    profile: impl Into<CompletionProfile>,
) -> Gate {
    let profile = profile.into();
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
    if profile == CompletionProfile::Timestamp {
        return verify_timestamp_token(cms, prepared);
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
    if profile == CompletionProfile::Signature(Profile::CadesB) {
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

/// `verifyTimestampToken`: a document timestamp's token must stamp the
/// prepared digest with the prepared algorithm, its own message digest must
/// cover its TSTInfo, and its signature must verify with the authority's
/// certificate.
fn verify_timestamp_token(cms: &[u8], prepared: &Prepared) -> Gate {
    let refuse = |reason, detail: String| Gate::Refused { reason, detail };
    let read = parse_cms_content(cms, CmsContent::TstInfo).and_then(|internal| {
        let info = read_timestamp_info(internal.e_content.as_deref().unwrap_or_default())?;
        Ok((internal, info))
    });
    let (internal, info) = match read {
        Ok(read) => read,
        Err(e) => return refuse("malformed", e.text()),
    };
    if info.imprint_algorithm.name() != prepared.algorithm {
        return refuse(
            "algorithm-mismatch",
            format!(
                "the token stamps a {} digest, the document was prepared with {}",
                info.imprint_algorithm.name(),
                prepared.algorithm
            ),
        );
    }
    if info.imprint != prepared.digest {
        return refuse(
            "digest-mismatch",
            "the token's message imprint is not the prepared digest".into(),
        );
    }
    let content_digest =
        internal.parsed.digest_algorithm.hash(internal.e_content.as_deref().unwrap_or_default());
    if content_digest != internal.parsed.message_digest {
        return refuse(
            "signature-invalid",
            "the token's message digest does not cover its TSTInfo".into(),
        );
    }
    match verify_parsed(&internal) {
        Cryptography::Valid => Gate::Ok,
        Cryptography::Unsupported => {
            refuse("signature-invalid", "the signature algorithm is not supported".into())
        }
        Cryptography::Invalid => {
            refuse("signature-invalid", "the token signature does not verify".into())
        }
    }
}
