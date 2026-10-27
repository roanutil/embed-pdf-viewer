//! A bundle as one JSON file, each resource in base64
//! (`core/src/transfer/codec.ts`, `AnnotationTransfer.ts`,
//! `FormTransfer.ts`, and `core/src/resource/base64.ts`): a bundle whole, to
//! store or download.

use serde_json::{Map, Value};

use super::annotation_bundle::{ANNOTATION_BUNDLE_KEYS, assert_annotation_bundle_manifest};
use super::bundle::{Bundle, BundleKind, assert_resource_ids};
use super::form_bundle::{FORM_BUNDLE_KEYS, assert_form_bundle_manifest};
use super::limits::{BundleLimits, past_limit};
use crate::error::{EngineError, EngineErrorCode};

/// A resource's entry in the file besides its base64: its id, quotes and
/// separators (`RESOURCE_ENTRY_CHARACTERS`).
const RESOURCE_ENTRY_CHARACTERS: u64 = 81;
/// The file's own keys and punctuation (`ENVELOPE_CHARACTERS`).
const ENVELOPE_CHARACTERS: u64 = 256;

fn keys_of(kind: BundleKind) -> &'static [&'static str] {
    match kind {
        BundleKind::Annotation => ANNOTATION_BUNDLE_KEYS,
        BundleKind::Form => FORM_BUNDLE_KEYS,
    }
}

fn assert_manifest(
    kind: BundleKind,
    manifest: &Map<String, Value>,
    sizes: &[(String, usize)],
    limits: &BundleLimits,
) -> Result<(), EngineError> {
    match kind {
        BundleKind::Annotation => assert_annotation_bundle_manifest(manifest, sizes, limits),
        BundleKind::Form => assert_form_bundle_manifest(manifest, sizes, limits),
    }
}

/// The file for `bundle` (`BundleCodec.stringify`). Its shape, references
/// and limits are checked first, so no file is written that [`parse`]
/// refuses for them; the resources are checked against their ids by
/// `parse`.
pub fn stringify(
    kind: BundleKind,
    bundle: &Bundle,
    limits: &BundleLimits,
) -> Result<String, EngineError> {
    assert_manifest(kind, &bundle.manifest, &bundle.sizes(), limits)?;
    let mut file = Map::new();
    for key in keys_of(kind) {
        if let Some(value) = bundle.manifest.get(*key) {
            file.insert((*key).to_owned(), value.clone());
        }
    }
    let resources: Map<String, Value> = bundle
        .resources
        .iter()
        .map(|(id, bytes)| (id.clone(), Value::String(to_base64(bytes))))
        .collect();
    file.insert("resources".into(), Value::Object(resources));
    serde_json::to_string(&Value::Object(file)).map_err(|e| {
        EngineError::with_message(
            EngineErrorCode::Unknown,
            format!("the bundle did not serialize: {e}"),
        )
    })
}

/// The bundle a file holds, checked whole (`BundleCodec.parse`):
/// `InvalidArg` for anything that isn't a bundle of this family and
/// version, a resource that doesn't match its id, a row naming a resource
/// that isn't there, or a resource no row names; past a limit, the limit's
/// refusal. Limits come first, so a file that is refused is never decoded:
/// the text's length, then the counts and every resource's size from its
/// base64, and only then the resources, decoded and hashed.
pub fn parse(kind: BundleKind, text: &str, limits: &BundleLimits) -> Result<Bundle, EngineError> {
    let max_length = (limits.bundle_bytes * 4).div_ceil(3)
        + limits.resources * RESOURCE_ENTRY_CHARACTERS
        + ENVELOPE_CHARACTERS;
    // `text.length`: UTF-16 code units, as JavaScript counts them.
    let length: u64 = text.chars().map(|c| c.len_utf16() as u64).sum();
    if length > max_length {
        return Err(past_limit(
            format!(
                "the file is {length} characters, more than a bundle within {} bytes can be",
                limits.bundle_bytes
            ),
            serde_json::json!({ "limit": "bundleBytes", "max": limits.bundle_bytes,
                "fileLength": length }),
        ));
    }
    let not_a_bundle = || {
        EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!("the file is not {}", kind.a_bundle()),
        )
    };
    let file: Value = serde_json::from_str(text).map_err(|_| not_a_bundle())?;
    let Some(file) = file.as_object() else { return Err(not_a_bundle()) };
    let Some(encoded) = file.get("resources").and_then(Value::as_object) else {
        return Err(not_a_bundle());
    };
    let not_base64 = |id: &str| {
        EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!("resource {id} is not base64"),
        )
    };
    let mut sizes = Vec::new();
    for (id, value) in encoded {
        let size = value.as_str().and_then(decoded_length_of).ok_or_else(|| not_base64(id))?;
        sizes.push((id.clone(), size));
    }
    assert_manifest(kind, file, &sizes, limits)?;

    let mut resources = Vec::new();
    for (id, value) in encoded {
        let bytes = value.as_str().and_then(from_base64).ok_or_else(|| not_base64(id))?;
        resources.push((id.clone(), bytes));
    }
    assert_resource_ids(kind, &resources)?;
    let mut manifest = Map::new();
    for key in keys_of(kind) {
        if let Some(value) = file.get(*key) {
            manifest.insert((*key).to_owned(), value.clone());
        }
    }
    Ok(Bundle { manifest, resources })
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Padded base64 of `bytes` (`toBase64`).
pub fn to_base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let triple = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(ALPHABET[(triple >> 18) as usize] as char);
        out.push(ALPHABET[((triple >> 12) & 0x3f) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[((triple >> 6) & 0x3f) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 { ALPHABET[(triple & 0x3f) as usize] as char } else { '=' });
    }
    out
}

/// Trailing padding, scanned backward (`unpaddedLengthOf`).
fn unpadded_length_of(encoded: &str) -> usize {
    encoded.trim_end_matches('=').len()
}

/// The number of bytes `encoded` decodes to, without decoding it, or `None`
/// when it can't be base64 (`decodedLengthOf`). Padding is optional.
pub fn decoded_length_of(encoded: &str) -> Option<usize> {
    let end = unpadded_length_of(encoded);
    (end % 4 != 1).then_some(end * 3 / 4)
}

/// The bytes padded or unpadded base64 holds; `None` for anything else
/// (`fromBase64`).
pub fn from_base64(encoded: &str) -> Option<Vec<u8>> {
    let end = unpadded_length_of(encoded);
    if end % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(end * 3 / 4);
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for byte in &encoded.as_bytes()[..end] {
        let value = ALPHABET.iter().position(|c| c == byte)? as u32;
        buffer = ((buffer << 6) | value) & 0x00ff_ffff;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buffer >> bits) & 0xff) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    //! `core/test/transfer/AnnotationTransfer.test.ts` and the file half of
    //! `core/test/transfer/formTransfer.test.ts`.

    use super::*;
    use crate::features::transfer::bundle::resource_id_of;
    use crate::features::transfer::limits::DEFAULT_BUNDLE_LIMITS;
    use serde_json::json;

    const DRAWING: &[u8] = b"%PDF-1.7 a drawing";
    const FILE: &[u8] = b"an attached file";

    fn page() -> Value {
        json!({ "kind": "objectNumber", "objectNumber": 3 })
    }

    fn stamp(annot: u32) -> Value {
        json!({
            "subtype": "stamp",
            "ref": { "kind": "objectNumber", "page": page(), "objectNumber": annot },
            "rect": { "x": 10, "y": 10, "width": 100, "height": 50 },
            "contents": "Apprové ✓",
        })
    }

    /// Two stamps sharing one drawing and an attachment with its file.
    fn sample() -> Bundle {
        let drawing = resource_id_of(DRAWING).unwrap();
        let file = resource_id_of(FILE).unwrap();
        let manifest = json!({
            "format": "embedpdf/annotations",
            "version": 1,
            "pages": [{ "page": page(), "position": 0, "size": { "width": 612, "height": 792 } }],
            "items": [
                { "data": stamp(12), "resources": { "appearance": drawing } },
                { "data": stamp(13), "resources": { "appearance": drawing } },
                { "data": { "subtype": "file-attachment",
                    "ref": { "kind": "objectNumber", "page": page(), "objectNumber": 14 } },
                  "resources": { "file": file } },
            ],
        });
        Bundle {
            manifest: manifest.as_object().unwrap().clone(),
            resources: vec![(drawing, DRAWING.to_vec()), (file, FILE.to_vec())],
        }
    }

    const ANNOTATION: BundleKind = BundleKind::Annotation;

    fn text_of(bundle: &Bundle) -> String {
        stringify(ANNOTATION, bundle, &DEFAULT_BUNDLE_LIMITS).unwrap()
    }

    /// The file for `bundle`, with `change` applied to its JSON.
    fn edited(bundle: &Bundle, change: impl FnOnce(&mut Value)) -> String {
        let mut json: Value = serde_json::from_str(&text_of(bundle)).unwrap();
        change(&mut json);
        json.to_string()
    }

    fn limits_with(change: impl FnOnce(&mut BundleLimits)) -> BundleLimits {
        let mut limits = DEFAULT_BUNDLE_LIMITS;
        change(&mut limits);
        limits
    }

    fn refusal(text: &str, limits: &BundleLimits) -> EngineError {
        parse(ANNOTATION, text, limits).unwrap_err()
    }

    #[test]
    fn a_bundle_comes_back_through_the_file_as_the_same_bundle() {
        let bundle = sample();
        let text = text_of(&bundle);
        assert_eq!(parse(ANNOTATION, &text, &DEFAULT_BUNDLE_LIMITS).unwrap(), bundle);
        // A resource once however many items name it.
        let json: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(json["resources"].as_object().unwrap().len(), 2);
        // Its keys in the order a file writes them.
        let keys: Vec<&String> = json.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["format", "version", "pages", "items", "resources"]);
    }

    #[test]
    fn parse_refuses_a_file_that_is_not_a_bundle() {
        for text in ["not json", r#"{"format":"embedpdf/annotations"}"#] {
            let refused = refusal(text, &DEFAULT_BUNDLE_LIMITS);
            assert_eq!(refused.code, EngineErrorCode::InvalidArg);
            assert!(refused.message.contains("not an annotation bundle"), "{refused}");
        }
        let bundle = sample();
        for (field, value) in [("format", json!("embedpdf/other")), ("version", json!(2))] {
            let refused =
                refusal(&edited(&bundle, |json| json[field] = value), &DEFAULT_BUNDLE_LIMITS);
            assert_eq!(refused.code, EngineErrorCode::InvalidArg);
            assert!(refused.message.contains(&format!("unknown {field}")), "{refused}");
        }
        let refused = refusal(
            &edited(&bundle, |json| json["comment"] = json!("hello")),
            &DEFAULT_BUNDLE_LIMITS,
        );
        assert!(refused.message.contains("unknown field 'comment'"), "{refused}");
        let refused = refusal(
            &edited(&bundle, |json| json["pages"][0]["page"]["objectNumber"] = json!(4)),
            &DEFAULT_BUNDLE_LIMITS,
        );
        assert!(refused.message.contains("page the bundle doesn't list"), "{refused}");
    }

    #[test]
    fn parse_checks_resources_against_items_and_ids() {
        let bundle = sample();
        let drawing = bundle.resources[0].0.clone();
        let mut tampered = DRAWING.to_vec();
        tampered[0] ^= 1;
        let cases = [
            (
                edited(&bundle, |json| json["resources"][&drawing] = json!(to_base64(&tampered))),
                "doesn't match its id",
            ),
            (
                edited(&bundle, |json| {
                    json["resources"].as_object_mut().unwrap().remove(&drawing);
                }),
                "which the bundle doesn't hold",
            ),
            (
                edited(&bundle, |json| {
                    let extra = resource_id_of(b"unused").unwrap();
                    json["resources"][extra] = json!(to_base64(b"unused"));
                }),
                "is named by no item",
            ),
            (
                edited(&bundle, |json| json["resources"][&drawing] = json!("not!base64")),
                "is not base64",
            ),
        ];
        for (text, message) in cases {
            let refused = refusal(&text, &DEFAULT_BUNDLE_LIMITS);
            assert_eq!(refused.code, EngineErrorCode::InvalidArg, "{message}");
            assert!(refused.message.contains(message), "{message}: {refused}");
        }
    }

    #[test]
    fn parse_applies_the_limits_before_it_decodes_anything() {
        let limits = limits_with(|l| {
            l.bundle_bytes = 1_000;
            l.resources = 1;
        });
        let refused = refusal(&"x".repeat(2_000), &limits);
        assert_eq!(refused.details.unwrap()["limit"], "bundleBytes");

        let text = text_of(&sample());
        type Change = Box<dyn Fn(&mut BundleLimits)>;
        let cases: [(&str, Change, u64); 6] = [
            ("items", Box::new(|l| l.items = 2), 2),
            ("pages", Box::new(|l| l.pages = 0), 0),
            ("resources", Box::new(|l| l.resources = 1), 1),
            (
                "resourceBytes",
                Box::new(|l| l.resource_bytes = DRAWING.len() as u64 - 1),
                DRAWING.len() as u64 - 1,
            ),
            ("manifestBytes", Box::new(|l| l.manifest_bytes = 100), 100),
            ("bundleBytes", Box::new(|l| l.bundle_bytes = 600), 600),
        ];
        for (limit, change, max) in cases {
            let refused = refusal(&text, &limits_with(change));
            let details = refused.details.unwrap();
            assert_eq!(details["limit"], limit);
            assert_eq!(details["max"], max);
        }
        let exactly = limits_with(|l| {
            l.items = 3;
            l.pages = 1;
            l.resources = 2;
            l.resource_bytes = DRAWING.len() as u64;
        });
        assert_eq!(parse(ANNOTATION, &text, &exactly).unwrap(), sample());
    }

    #[test]
    fn stringify_writes_no_file_parse_would_refuse_for_its_shape_or_size() {
        let refused = stringify(ANNOTATION, &sample(), &limits_with(|l| l.items = 1)).unwrap_err();
        assert_eq!(refused.details.unwrap()["limit"], "items");
        let missing = Bundle { resources: Vec::new(), ..sample() };
        let refused = stringify(ANNOTATION, &missing, &DEFAULT_BUNDLE_LIMITS).unwrap_err();
        assert!(refused.message.contains("which the bundle doesn't hold"), "{refused}");
    }

    #[test]
    fn base64_round_trips_with_or_without_padding() {
        for bytes in [&b""[..], b"a", b"ab", b"abc", b"abcd", &[0u8, 255, 128, 7][..]] {
            let encoded = to_base64(bytes);
            assert_eq!(from_base64(&encoded).as_deref(), Some(bytes));
            assert_eq!(from_base64(encoded.trim_end_matches('=')).as_deref(), Some(bytes));
            assert_eq!(decoded_length_of(&encoded), Some(bytes.len()));
        }
        assert_eq!(to_base64(b"%FDF-1.2"), "JUZERi0xLjI=");
        assert_eq!(from_base64("a"), None);
        assert_eq!(decoded_length_of("abcde"), None);
    }
}
