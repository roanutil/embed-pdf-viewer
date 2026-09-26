//! `convention.ts`: Acrobat's stamp-library convention, interpreted in one
//! place. A library's `/Names /Pages` registry names each stamp page with a
//! key `identifier=label`; `/PieceInfo` carries only what has no standard
//! home.

use serde_json::{Value, json};

use crate::contract::StampAssetKind;

/// `StampKey`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StampKey {
    pub name: String,
    pub label: String,
}

/// `parseStampKey(key)`: split at the first `=`; a key without one is its
/// own label.
pub fn parse_stamp_key(key: &str) -> StampKey {
    match key.find('=') {
        None => StampKey { name: key.to_string(), label: key.to_string() },
        Some(at) => StampKey { name: key[..at].to_string(), label: key[at + 1..].to_string() },
    }
}

/// `stampKey(name, label)`.
pub fn stamp_key(name: &str, label: &str) -> String {
    format!("{name}={label}")
}

const BASE62: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

/// `customStampName(random)`: `#` and 22 base-62 characters, Acrobat's own
/// shape.
pub fn custom_stamp_name(random: &dyn Fn() -> f64) -> String {
    let mut out = String::from("#");
    for _ in 0..22 {
        let i = (random() * BASE62.len() as f64).floor() as usize;
        out.push(BASE62[i.min(BASE62.len() - 1)] as char);
    }
    out
}

/// `assetIdFor(libraryId, name)`.
pub fn asset_id_for(library_id: &str, name: &str) -> String {
    format!("{library_id}:{name}")
}

/// Catalog `/PieceInfo` application name for library-level data.
pub const STAMP_LIBRARY_PIECEINFO_APP: &str = "EMBD_StampLibrary";
/// Page `/PieceInfo` application name for per-stamp data.
pub const STAMP_PIECEINFO_APP: &str = "EMBD_Stamp";
/// The `Version` both dictionaries carry.
pub const STAMP_PIECEINFO_VERSION: u32 = 2;
/// The library kind every file has unless it says otherwise.
pub const DEFAULT_LIBRARY_KIND: &str = "stamps";

/// `libraryKindToPdfName(kind)`.
pub fn library_kind_to_pdf_name(kind: &str) -> String {
    match kind {
        "stamps" => "StampLibrary".into(),
        "signatures" => "SignatureLibrary".into(),
        other => other.into(),
    }
}

/// `libraryKindFromPdfName(name)`.
pub fn library_kind_from_pdf_name(name: Option<&str>) -> String {
    match name {
        None | Some("") | Some("StampLibrary") => DEFAULT_LIBRARY_KIND.into(),
        Some("SignatureLibrary") => "signatures".into(),
        Some(other) => other.into(),
    }
}

/// `stampKindToPdfName(kind)`.
pub fn stamp_kind_to_pdf_name(kind: StampAssetKind) -> &'static str {
    match kind {
        StampAssetKind::Signature => "Signature",
        StampAssetKind::Initials => "Initials",
        StampAssetKind::Stamp => "Stamp",
    }
}

fn strings_or_null(values: Option<&[String]>) -> Value {
    values.map_or(Value::Null, |v| json!(v))
}

/// `stampLibraryPieceInfo(id, {kind, categories, locale})`.
pub fn stamp_library_piece_info(
    id: &str,
    kind: Option<&str>,
    categories: Option<&[String]>,
    locale: Option<&str>,
) -> Value {
    json!({
        "Version": STAMP_PIECEINFO_VERSION,
        "Id": id,
        "Kind": { "name": library_kind_to_pdf_name(kind.unwrap_or(DEFAULT_LIBRARY_KIND)) },
        "Name": null,
        "Categories": strings_or_null(categories),
        "Locale": locale.map_or(Value::Null, |l| json!(l)),
    })
}

/// `stampPieceInfo(kind, {subject, categories})`.
pub fn stamp_piece_info(
    kind: StampAssetKind,
    subject: Option<&str>,
    categories: Option<&[String]>,
) -> Value {
    json!({
        "Version": STAMP_PIECEINFO_VERSION,
        "Kind": { "name": stamp_kind_to_pdf_name(kind) },
        "Name": null,
        "Subject": null,
        "SubjectOverride": subject.map_or(Value::Null, |s| json!(s)),
        "Categories": strings_or_null(categories),
    })
}
