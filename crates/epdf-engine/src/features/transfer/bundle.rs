//! What every bundle shares, whichever family it carries
//! (`core/src/transfer/bundle.ts`): resources named by the SHA-256 of their
//! bytes, the table of pages its rows are on, the checks of that shape, and
//! where an import puts each page.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::error::{EngineError, EngineErrorCode};
use crate::identity::PageRef;

/// What a bundle holds: annotations, or form fields with their widgets
/// (`BundleKind`). Messages name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BundleKind {
    Annotation,
    Form,
}

impl BundleKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Annotation => "annotation",
            Self::Form => "form",
        }
    }

    /// `A_BUNDLE`: how a refusal of a file names the family.
    pub fn a_bundle(self) -> &'static str {
        match self {
            Self::Annotation => "an annotation bundle",
            Self::Form => "a form bundle",
        }
    }
}

/// A bundle: every key of its manifest but `resources`, in the order a file
/// writes them, and its resources by id, in order. The rows stay the JSON
/// objects a read returns (`AnnotationBundle`, `FormBundle`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Bundle {
    pub manifest: Map<String, Value>,
    pub resources: Vec<(String, Vec<u8>)>,
}

impl Bundle {
    /// Each resource's size by id, for the checks that need no bytes.
    pub fn sizes(&self) -> Vec<(String, usize)> {
        self.resources.iter().map(|(id, bytes)| (id.clone(), bytes.len())).collect()
    }

    /// The bytes of resource `id`.
    pub fn resource(&self, id: &str) -> Option<&[u8]> {
        self.resources.iter().find(|(key, _)| key == id).map(|(_, bytes)| bytes.as_slice())
    }
}

/// A page a bundle's rows are on or point at (`BundlePage`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BundlePage {
    pub page: PageRef,
    /// Where the page is in its document, for mapping pages by position;
    /// never a reference.
    pub position: u32,
    /// The page's size, as its layout reports it: positions are measured
    /// from the page's top-left.
    pub size: crate::geom::PdfSize,
}

/// One pair of a page list (`{ from, to }`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PagePair {
    pub from: PageRef,
    pub to: PageRef,
}

/// Where an import puts each page of the bundle (`BundleImportPages`):
/// each page as it is (`'same'`, the default), a page to the target's page
/// at its `position` (`'by-position'`), or exactly the pairs given.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum BundleImportPages {
    #[default]
    Same,
    ByPosition,
    List(Vec<PagePair>),
}

impl Serialize for BundleImportPages {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Same => s.serialize_str("same"),
            Self::ByPosition => s.serialize_str("by-position"),
            Self::List(pairs) => pairs.serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for BundleImportPages {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        match Value::deserialize(d)? {
            Value::String(name) if name == "same" => Ok(Self::Same),
            Value::String(name) if name == "by-position" => Ok(Self::ByPosition),
            list @ Value::Array(_) => {
                Vec::<PagePair>::deserialize(list).map(Self::List).map_err(D::Error::custom)
            }
            other => Err(D::Error::custom(format!(
                "pages is 'same', 'by-position' or a list of {{ from, to }}, not {other}"
            ))),
        }
    }
}

/// A page of the document an import goes into: its ref and where it is
/// (`BundleImportTarget`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BundleImportTarget {
    pub page: PageRef,
    pub position: u32,
}

/// `encodePageKey`.
pub fn page_key(page: PageRef) -> String {
    format!("obj:{}", page.object_number().get())
}

/// `sha256-` and 64 lowercase hex digits (`RESOURCE_ID_PATTERN`).
pub fn is_resource_id(id: &str) -> bool {
    id.strip_prefix("sha256-").is_some_and(|hex| {
        hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

/// The id of a resource: `sha256-` and the SHA-256 of its bytes
/// (`resourceIdOf`, `sha256HexOf`).
pub fn resource_id_of(bytes: &[u8]) -> Result<String, EngineError> {
    let digest = pdfium::sha256(bytes).ok_or_else(|| {
        EngineError::with_message(EngineErrorCode::Unknown, "EPDF_DigestBuffer refused")
    })?;
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(format!("sha256-{hex}"))
}

/// Every resource's bytes hash to its id (`assertResourceIds`).
pub fn assert_resource_ids(
    kind: BundleKind,
    resources: &[(String, Vec<u8>)],
) -> Result<(), EngineError> {
    for (id, bytes) in resources {
        if resource_id_of(bytes)? != *id {
            return Err(invalid_bundle(kind, &format!("resource {id} doesn't match its id")));
        }
    }
    Ok(())
}

/// Refuse a bundle that isn't one: `InvalidArg`, naming its kind
/// (`invalidBundle`).
pub fn invalid_bundle(kind: BundleKind, message: &str) -> EngineError {
    EngineError::with_message(
        EngineErrorCode::InvalidArg,
        format!("{} bundle: {message}", kind.as_str()),
    )
}

/// `value` with every number spelled as JavaScript spells it: a whole
/// number without a fraction (`612`, not `612.0`). A bundle leaves the
/// engine this way, so its JSON is the TypeScript engine's byte for byte:
/// a read measures in `f64`, which serializes `612.0`, and
/// `JSON.stringify` writes `612`.
pub fn js_numbers(value: Value) -> Value {
    match value {
        Value::Number(number) => match number.as_f64() {
            Some(n) if number.is_f64() && n.fract() == 0.0 && n.abs() < 9_007_199_254_740_992.0 => {
                // `-0` is `0` to `JSON.stringify`.
                if n < 0.0 { Value::from(n as i64) } else { Value::from(n as u64) }
            }
            _ => Value::Number(number),
        },
        Value::Array(items) => Value::Array(items.into_iter().map(js_numbers).collect()),
        Value::Object(fields) => {
            Value::Object(fields.into_iter().map(|(key, field)| (key, js_numbers(field))).collect())
        }
        other => other,
    }
}

/// `isRecord`: an object, not an array or `null`.
pub fn record(value: &Value) -> Option<&Map<String, Value>> {
    value.as_object()
}

/// `JSON.stringify` of a value, for a message.
pub fn json_text(value: Option<&Value>) -> String {
    value.map_or_else(|| "undefined".to_owned(), Value::to_string)
}

/// A page ref as a bundle holds one: by object number, a positive integer
/// (`isBundlePageRef`).
pub fn bundle_page_ref(value: Option<&Value>) -> Option<PageRef> {
    let page = value?.as_object()?;
    if page.get("kind").and_then(Value::as_str) != Some("objectNumber") {
        return None;
    }
    let number = whole_number(page.get("objectNumber")?)?;
    let number = u32::try_from(number).ok().filter(|n| *n > 0)?;
    crate::identity::PageObjectNumber::new(number).ok().map(PageRef::new)
}

/// `Number.isInteger`, for a JSON number: `3` and `3.0` both are.
pub fn whole_number(value: &Value) -> Option<u64> {
    let number = value.as_number()?;
    number.as_u64().or_else(|| {
        number
            .as_f64()
            .filter(|n| n.fract() == 0.0 && *n >= 0.0 && *n < 2f64.powi(53))
            .map(|n| n as u64)
    })
}

fn is_size(value: Option<&Value>) -> bool {
    let Some(size) = value.and_then(Value::as_object) else { return false };
    [size.get("width"), size.get("height")]
        .into_iter()
        .all(|length| length.and_then(Value::as_f64).is_some_and(|n| n.is_finite() && n >= 0.0))
}

/// Check a bundle's page table: each entry a page ref, a position and a
/// size, no page or position listed twice. Answers the listed pages' keys,
/// for the rows to be checked against (`assertBundlePages`).
pub fn assert_bundle_pages(
    kind: BundleKind,
    pages: &[Value],
) -> Result<HashSet<String>, EngineError> {
    let mut keys = HashSet::new();
    let mut positions = HashSet::new();
    for entry in pages {
        let Some(fields) = record(entry) else {
            return Err(invalid_bundle(kind, "a page needs a page ref, a position and a size"));
        };
        let Some(page) = bundle_page_ref(fields.get("page")) else {
            return Err(invalid_bundle(kind, "a page needs a page ref, a position and a size"));
        };
        if !is_size(fields.get("size")) {
            return Err(invalid_bundle(kind, "a page needs a page ref, a position and a size"));
        }
        assert_known_fields(kind, "a page", fields, &["page", "position", "size"])?;
        let position = fields.get("position");
        let Some(at) = position.and_then(whole_number) else {
            return Err(invalid_bundle(
                kind,
                &format!("page position {} is not a position", json_text(position)),
            ));
        };
        let key = page_key(page);
        if keys.contains(&key) {
            return Err(invalid_bundle(kind, &format!("page {key} is listed twice")));
        }
        if positions.contains(&at) {
            return Err(invalid_bundle(kind, &format!("position {at} is listed twice")));
        }
        keys.insert(key);
        positions.insert(at);
    }
    Ok(keys)
}

/// Check the resources a row names, by role: each role one of `roles`, each
/// id a resource id the bundle holds. Each id is added to `named`
/// (`assertRowResources`).
pub fn assert_row_resources(
    kind: BundleKind,
    row: &str,
    resources: Option<&Value>,
    roles: &[&str],
    sizes: &[(String, usize)],
    named: &mut HashSet<String>,
) -> Result<(), EngineError> {
    let Some(resources) = resources.and_then(Value::as_object) else {
        return Err(invalid_bundle(kind, &format!("{row} has no resources")));
    };
    for (role, id) in resources {
        if !roles.contains(&role.as_str()) {
            return Err(invalid_bundle(
                kind,
                &format!("{row} names an unknown resource role '{role}'"),
            ));
        }
        let Some(id) = id.as_str().filter(|id| is_resource_id(id)) else {
            return Err(invalid_bundle(
                kind,
                &format!("{row} names {id}, which is not a resource id"),
            ));
        };
        if !sizes.iter().any(|(held, _)| held == id) {
            return Err(invalid_bundle(
                kind,
                &format!("{row} names {id}, which the bundle doesn't hold"),
            ));
        }
        named.insert(id.to_owned());
    }
    Ok(())
}

/// Every resource the bundle holds has a resource id and is named by one of
/// its `rows` (`assertResourcesNamed`).
pub fn assert_resources_named(
    kind: BundleKind,
    rows: &str,
    sizes: &[(String, usize)],
    named: &HashSet<String>,
) -> Result<(), EngineError> {
    for (id, _) in sizes {
        if !is_resource_id(id) {
            return Err(invalid_bundle(
                kind,
                &format!("{} is not a resource id", Value::from(id.as_str())),
            ));
        }
        if !named.contains(id) {
            return Err(invalid_bundle(kind, &format!("resource {id} is named by no {rows}")));
        }
    }
    Ok(())
}

/// `value` has no field outside `fields` (`assertKnownFields`).
pub fn assert_known_fields(
    kind: BundleKind,
    what: &str,
    value: &Map<String, Value>,
    fields: &[&str],
) -> Result<(), EngineError> {
    match value.keys().find(|field| !fields.contains(&field.as_str())) {
        Some(field) => Err(invalid_bundle(kind, &format!("{what} has an unknown field '{field}'"))),
        None => Ok(()),
    }
}

/// Where an import puts each page of a bundle, worked out before the first
/// write (`bundlePageMapping`).
#[derive(Debug, Clone)]
pub struct PageMap {
    table: HashMap<String, PageRef>,
}

impl PageMap {
    /// Where `page` goes. Every page the rows name was checked to map
    /// somewhere; one that wasn't stays as it is.
    pub fn map(&self, page: PageRef) -> PageRef {
        self.table.get(&page_key(page)).copied().unwrap_or(page)
    }
}

/// Where an import puts each page of a bundle, as `pages` says, among the
/// `target` document's pages. Every page in `named` (the pages the rows are
/// on or point at) must map somewhere: `InvalidArg` names those that don't,
/// and a list that maps to a page the target doesn't have is `NotFound`.
/// Nothing here touches a document, so an import refuses before its first
/// write (`bundlePageMapping`).
pub fn bundle_page_mapping(
    bundle_pages: &[BundlePage],
    pages: &BundleImportPages,
    target: &[BundleImportTarget],
    named: &[PageRef],
) -> Result<PageMap, EngineError> {
    let target_by_key: HashMap<String, PageRef> =
        target.iter().map(|entry| (page_key(entry.page), entry.page)).collect();
    let mut table = HashMap::new();
    match pages {
        BundleImportPages::Same => {
            for entry in bundle_pages {
                if let Some(to) = target_by_key.get(&page_key(entry.page)) {
                    table.insert(page_key(entry.page), *to);
                }
            }
        }
        BundleImportPages::ByPosition => {
            let by_position: HashMap<u32, PageRef> =
                target.iter().map(|entry| (entry.position, entry.page)).collect();
            for entry in bundle_pages {
                if let Some(to) = by_position.get(&entry.position) {
                    table.insert(page_key(entry.page), *to);
                }
            }
        }
        BundleImportPages::List(pairs) => {
            for pair in pairs {
                let key = page_key(pair.from);
                if table.contains_key(&key) {
                    return Err(EngineError::with_message(
                        EngineErrorCode::InvalidArg,
                        format!("import: page {key} is mapped twice"),
                    ));
                }
                let Some(found) = target_by_key.get(&page_key(pair.to)) else {
                    return Err(EngineError::with_message(
                        EngineErrorCode::NotFound,
                        format!("import: the document has no page {}", page_key(pair.to)),
                    ));
                };
                table.insert(key, *found);
            }
        }
    }
    let mut unmapped: Vec<String> = Vec::new();
    for page in named {
        let key = page_key(*page);
        if !table.contains_key(&key) && !unmapped.contains(&key) {
            unmapped.push(key);
        }
    }
    if !unmapped.is_empty() {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!(
                "import: {} {} map to no page of this document",
                if unmapped.len() == 1 { "page" } else { "pages" },
                unmapped.join(", ")
            ),
        )
        .with_details(serde_json::json!({ "pages": unmapped })));
    }
    Ok(PageMap { table })
}

/// The bundle's page table, read: each entry was checked by
/// [`assert_bundle_pages`].
pub fn bundle_pages_of(manifest: &Map<String, Value>) -> Result<Vec<BundlePage>, EngineError> {
    serde_json::from_value(manifest.get("pages").cloned().unwrap_or(Value::Array(Vec::new())))
        .map_err(|e| {
            EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!("the bundle's pages are not pages: {e}"),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::PageObjectNumber;

    fn page(number: u32) -> PageRef {
        PageRef::new(PageObjectNumber::new(number).unwrap())
    }

    #[test]
    fn a_resource_is_named_by_the_sha256_of_its_bytes() {
        assert_eq!(
            resource_id_of(b"abc").unwrap(),
            "sha256-ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert!(is_resource_id(&resource_id_of(b"abc").unwrap()));
        assert!(!is_resource_id("sha256-ABC"));
    }

    #[test]
    fn pages_map_as_they_are_by_position_or_as_a_list_says() {
        let bundle = vec![
            BundlePage {
                page: page(3),
                position: 0,
                size: crate::geom::PdfSize { width: 1.0, height: 1.0 },
            },
            BundlePage {
                page: page(7),
                position: 4,
                size: crate::geom::PdfSize { width: 1.0, height: 1.0 },
            },
        ];
        let same = [
            BundleImportTarget { page: page(3), position: 0 },
            BundleImportTarget { page: page(7), position: 4 },
        ];
        let mapped =
            bundle_page_mapping(&bundle, &BundleImportPages::Same, &same, &[page(3)]).unwrap();
        assert_eq!(mapped.map(page(3)), page(3));
        let other = [
            BundleImportTarget { page: page(50), position: 0 },
            BundleImportTarget { page: page(51), position: 4 },
        ];
        let by_position =
            bundle_page_mapping(&bundle, &BundleImportPages::ByPosition, &other, &[page(7)])
                .unwrap();
        assert_eq!(by_position.map(page(7)), page(51));

        let refused =
            bundle_page_mapping(&bundle, &BundleImportPages::Same, &other, &[page(3), page(7)])
                .unwrap_err();
        assert_eq!(refused.code, EngineErrorCode::InvalidArg);
        assert_eq!(refused.details, Some(serde_json::json!({ "pages": ["obj:3", "obj:7"] })));

        let twice = BundleImportPages::List(vec![
            PagePair { from: page(3), to: page(50) },
            PagePair { from: page(3), to: page(50) },
        ]);
        assert_eq!(
            bundle_page_mapping(&bundle, &twice, &other, &[]).unwrap_err().code,
            EngineErrorCode::InvalidArg
        );
        let nowhere = BundleImportPages::List(vec![PagePair { from: page(3), to: page(99) }]);
        assert_eq!(
            bundle_page_mapping(&bundle, &nowhere, &other, &[]).unwrap_err().code,
            EngineErrorCode::NotFound
        );
    }

    #[test]
    fn import_pages_read_as_the_wire_spells_them() {
        let read = |value: Value| serde_json::from_value::<BundleImportPages>(value);
        assert_eq!(read(serde_json::json!("same")).unwrap(), BundleImportPages::Same);
        assert_eq!(read(serde_json::json!("by-position")).unwrap(), BundleImportPages::ByPosition);
        assert!(read(serde_json::json!("sideways")).is_err());
        let list = serde_json::json!([{ "from": { "kind": "objectNumber", "objectNumber": 3 },
            "to": { "kind": "objectNumber", "objectNumber": 4 } }]);
        assert_eq!(serde_json::to_value(read(list.clone()).unwrap()).unwrap(), list);
    }
}
