//! `verdict.ts`: integrity, cryptography, trust and modifications for every
//! signature of a document.

use std::rc::Rc;

use epdf_kernel::Error;
use epdf_kernel::bytes::Bytes;
use epdf_kernel::engine::DocumentHandle;
use serde_json::{Map, Value, json};

use crate::cms::{Cryptography, verify_parsed};
use crate::parse::{Internal, ParseError, parse_cms_internal};
use crate::trust::{CmsInput, TrustPort, TrustStatus, ValidationTime, validate_chain};

/// `ValidateSignaturesOptions`.
#[derive(Clone, Default)]
pub struct ValidateOptions {
    pub trust: Option<Rc<dyn TrustPort>>,
    pub at: Option<ValidationTime>,
    /// `persisted` (default) or `working-copy`.
    pub until: Option<String>,
}

fn text(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

/// `validateSignatures`: one verdict per signed signature, as JSON.
pub async fn validate_signatures(
    doc: Rc<dyn DocumentHandle>,
    options: ValidateOptions,
) -> Result<Value, Error> {
    if !doc.has("signatures") {
        return Err(Error::new("this engine does not implement signatures"));
    }
    let snapshot = doc.call("signatures.list", None, vec![]).await?;
    let mut verdicts = Vec::new();
    for signature in
        snapshot.get("signatures").and_then(Value::as_array).cloned().unwrap_or_default()
    {
        if signature.get("signed").and_then(Value::as_bool) != Some(true) {
            continue;
        }
        verdicts.push(validate_one(&doc, &snapshot, &signature, &options).await?);
    }
    Ok(Value::Array(verdicts))
}

async fn validate_one(
    doc: &Rc<dyn DocumentHandle>,
    snapshot: &Value,
    signature: &Value,
    options: &ValidateOptions,
) -> Result<Value, Error> {
    let at = options.at.clone().unwrap_or(ValidationTime::Now);
    let mut integrity = "indeterminate";
    let mut cryptography = Cryptography::Unsupported;
    let mut trust = TrustStatus::Unknown;
    let mut trust_reason = None;
    let mut internal: Option<Rc<Internal>> = None;

    let chain_valid = snapshot.get("chainValid").and_then(Value::as_bool) == Some(true);
    let coverage = text(signature, "coverage");
    if !chain_valid || coverage.as_deref() == Some("malformed") {
        integrity = if chain_valid { "invalid" } else { "indeterminate" };
    } else {
        let field = signature.get("field").cloned().unwrap_or(Value::Null);
        let outcome: Result<(), Option<ParseError>> = async {
            let contents = doc
                .call("signatures.contents", None, vec![field.clone()])
                .await
                .map_err(|_| None)?;
            let bytes = Bytes::from_json(&contents).map(|b| b.0).unwrap_or_default();
            let parsed = Rc::new(parse_cms_internal(&bytes).map_err(Some)?);
            internal = Some(parsed.clone());
            let digest = doc
                .call(
                    "signatures.digest",
                    None,
                    vec![field, json!(parsed.parsed.digest_algorithm.name())],
                )
                .await
                .map_err(|_| None)?;
            let digest = Bytes::from_json(&digest).map(|b| b.0).unwrap_or_default();
            let matches = digest == parsed.parsed.message_digest;
            integrity = if coverage.as_deref() == Some("whole-revision") && matches {
                "valid"
            } else {
                "invalid"
            };
            Ok(())
        }
        .await;
        if let Err(error) = outcome {
            // A CMS this engine cannot read is not evidence of tampering.
            integrity = match error {
                Some(ParseError::Cms { reason: "unsupported", .. }) => "indeterminate",
                Some(ParseError::Cms { .. }) => "invalid",
                _ => "indeterminate",
            };
        }
    }

    if let Some(parsed) = &internal {
        cryptography = verify_parsed(parsed);
        let chain =
            validate_chain(CmsInput::Parsed(parsed.clone()), options.trust.clone(), at.clone())
                .await?;
        trust = chain.status;
        trust_reason = chain.reason;
    }

    let modifications =
        modifications_of(doc, snapshot, signature, options.until.as_deref().unwrap_or("persisted"))
            .await;
    let verdict =
        modifications.get("verdict").and_then(Value::as_str).unwrap_or_default().to_string();
    let summary = if integrity == "invalid"
        || cryptography == Cryptography::Invalid
        || verdict == "forbidden"
    {
        "invalid"
    } else if integrity == "indeterminate"
        || cryptography == Cryptography::Unsupported
        || verdict == "indeterminate"
    {
        "indeterminate"
    } else if trust == TrustStatus::Trusted {
        "valid"
    } else {
        "valid-untrusted"
    };

    let mut out = Map::new();
    out.insert("signature".into(), signature.clone());
    out.insert("integrity".into(), json!(integrity));
    out.insert("cryptography".into(), json!(cryptography.name()));
    out.insert("trust".into(), json!(trust.name()));
    if let Some(reason) = trust_reason {
        out.insert("trustReason".into(), json!(reason));
    }
    out.insert("validatedAt".into(), at.to_json());
    out.insert("modifications".into(), modifications);
    out.insert("summary".into(), json!(summary));
    out.insert("cms".into(), internal.as_ref().map_or(Value::Null, |i| i.parsed.to_json()));
    Ok(Value::Object(out))
}

async fn modifications_of(
    doc: &Rc<dyn DocumentHandle>,
    snapshot: &Value,
    signature: &Value,
    until: &str,
) -> Value {
    let revision = signature.get("revisionIndex").and_then(Value::as_f64);
    let Some(revision) = revision else {
        return json!({
            "verdict": "indeterminate",
            "detail": "the signature seals no whole revision",
            "basis": "persisted",
        });
    };
    let revisions = snapshot.get("revisions").and_then(Value::as_array).map_or(0, Vec::len) as f64;
    if until == "persisted" && revision == revisions - 1.0 {
        return json!({ "verdict": "unchanged", "basis": "persisted" });
    }
    let input = json!({ "since": { "signatureIndex": signature.get("index").cloned().unwrap_or(Value::Null) }, "until": until });
    match doc.call("signatures.analyze", None, vec![input]).await {
        Err(error) => json!({
            "verdict": "indeterminate",
            "detail": format!("analysis failed: {}", error.message),
            "basis": "persisted",
        }),
        Ok(analysis) => {
            let current = analysis.get("current").cloned().unwrap_or(Value::Null);
            let primary = current.get("primary").filter(|p| !p.is_null());
            let object =
                |p: &Value| p.get("objectNumber").map_or_else(|| "undefined".into(), value_text);
            let detail = match primary {
                Some(p) if text(p, "verdict").as_deref() == Some("forbidden") => Some(format!(
                    "object {}, {}{}",
                    object(p),
                    p.get("rule").map_or_else(|| "undefined".into(), value_text),
                    text(p, "detail")
                        .filter(|d| !d.is_empty())
                        .map(|d| format!(": {d}"))
                        .unwrap_or_default()
                )),
                Some(p) if text(p, "verdict").as_deref() == Some("incomplete") => Some(format!(
                    "object {}: {}",
                    object(p),
                    text(p, "detail").unwrap_or_else(|| "incomplete evidence".into())
                )),
                _ => None,
            };
            let mut out = Map::new();
            out.insert("verdict".into(), current.get("verdict").cloned().unwrap_or(Value::Null));
            if let Some(detail) = detail {
                out.insert("detail".into(), json!(detail));
            }
            out.insert(
                "basis".into(),
                analysis.get("basis").and_then(|b| b.get("source")).cloned().unwrap_or(Value::Null),
            );
            let later = analysis.get("later").cloned().unwrap_or(Value::Null);
            let count = later.get("revisionCount").and_then(Value::as_f64).unwrap_or(0.0);
            if count > 0.0 {
                out.insert("laterRevisions".into(), later["revisionCount"].clone());
            }
            if later
                .get("undoneObjectNumbers")
                .and_then(Value::as_array)
                .is_some_and(|a| !a.is_empty())
            {
                out.insert("undone".into(), json!(true));
            }
            Value::Object(out)
        }
    }
}

/// A value as a template literal prints it.
fn value_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => crate::number_text(n.as_f64().unwrap_or(0.0)),
        Value::Null => "null".into(),
        other => other.to_string(),
    }
}
