//! G1. Durable page and annotation identity.
//!
//! Replaces `core/src/identity/*`. The rule the whole module exists for: an
//! index is not identity. A page index shifts when pages are inserted, deleted
//! or reordered while a session is open, so the primary key is the PDF
//! indirect object number and the index is display metadata.

use serde::{Deserialize, Serialize};

use crate::error::{EngineError, EngineErrorCode};
use crate::revision::RevisionToken;

/// A page's PDF indirect object number, resolved by
/// `EPDFPage_GetObjectNumber`.
///
/// A newtype rather than a bare integer because the invariant is the point:
/// this is always `> 0`. `0` means the file put a direct (non-indirect) page
/// dictionary in the `/Pages` tree, which is [`EngineErrorCode::MalformedPdf`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PageObjectNumber(u32);

impl PageObjectNumber {
    /// Accepts a raw object number, rejecting the direct-object case.
    pub fn new(value: u32) -> Result<Self, EngineError> {
        if value == 0 {
            return Err(EngineError::with_message(
                EngineErrorCode::MalformedPdf,
                "page is a direct object: EPDFPage_GetObjectNumber returned 0",
            ));
        }
        Ok(Self(value))
    }

    pub fn get(self) -> u32 {
        self.0
    }
}

impl std::fmt::Display for PageObjectNumber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// How the wire addresses a page: `PageRef` in `core/src/identity/PageRef.ts`,
/// `{ kind: "objectNumber", objectNumber }` (595bdc6c7).
///
/// One kind on purpose, as in the TypeScript: a page without an object number
/// is refused at open and a display index is never an identity. The tag stays
/// so that a kind this build does not know is refused rather than read as an
/// object number, which is what `PageRefSchema` does on the validated
/// surfaces.
///
/// The engine's own records keep [`PageObjectNumber`] and spell it as a
/// `PageRef` only on the wire, through [`page_ref`], [`page_refs`] and
/// [`optional_page_ref`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(tag = "kind")]
pub enum PageRef {
    #[serde(rename = "objectNumber", rename_all = "camelCase")]
    ObjectNumber { object_number: PageObjectNumber },
}

/// Read by hand so that `objectNumber` must be a positive integer, as
/// `PageRefSchema` has it. A derived impl would read the transparent
/// [`PageObjectNumber`] and let `0` through, which names a direct page
/// dictionary rather than a page.
impl<'de> Deserialize<'de> for PageRef {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(tag = "kind")]
        enum Wire {
            #[serde(rename = "objectNumber", rename_all = "camelCase")]
            ObjectNumber { object_number: u32 },
        }
        let Wire::ObjectNumber { object_number } = Wire::deserialize(d)?;
        PageObjectNumber::new(object_number)
            .map(|object_number| Self::ObjectNumber { object_number })
            .map_err(|_| serde::de::Error::custom("objectNumber must be a positive integer, got 0"))
    }
}

impl PageRef {
    /// `toPageRef`.
    pub fn new(page: PageObjectNumber) -> Self {
        Self::ObjectNumber { object_number: page }
    }

    /// The object number the ref resolves to, which is what the session keys
    /// every page record on (`DocumentSession.resolvePageRef`).
    pub fn object_number(self) -> PageObjectNumber {
        match self {
            Self::ObjectNumber { object_number } => object_number,
        }
    }

    /// `PageRef` decoded from a request argument.
    ///
    /// Kept divergence (`3baec217b`, kept on 2026-10-15): a present but
    /// malformed ref (an unknown `kind`, a missing `objectNumber`, or one
    /// that is not a positive integer) is refused as `InvalidArg`, as
    /// `PageRefSchema` answers it on the validated surfaces. The TypeScript
    /// worker does not validate it at all. An unknown object number is not
    /// decided here: it resolves to `NotFound` where the page is opened, as
    /// `resolvePageRef` answers it.
    pub fn from_value(value: &serde_json::Value, name: &str) -> Result<Self, EngineError> {
        Self::deserialize(value).map_err(|e| {
            EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!("'{name}' is not a PageRef: {e}"),
            )
        })
    }
}

impl From<PageObjectNumber> for PageRef {
    fn from(page: PageObjectNumber) -> Self {
        Self::new(page)
    }
}

/// A page number an engine record holds and the wire spells as a `PageRef`.
///
/// Implemented for [`PageObjectNumber`] and for the bare `u32` some readers
/// carry straight from PDFium, so that a record keeps its own type and only
/// its wire spelling changes.
pub trait WirePage: Copy {
    fn to_page_ref(self) -> PageRef;
    fn from_page_ref(page: PageRef) -> Self;
}

impl WirePage for PageObjectNumber {
    fn to_page_ref(self) -> PageRef {
        PageRef::new(self)
    }
    fn from_page_ref(page: PageRef) -> Self {
        page.object_number()
    }
}

impl WirePage for u32 {
    fn to_page_ref(self) -> PageRef {
        PageRef::new(PageObjectNumber(self))
    }
    fn from_page_ref(page: PageRef) -> Self {
        page.object_number().get()
    }
}

/// `#[serde(with)]` for one page the wire spells as a `PageRef`.
pub mod page_ref {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use super::{PageRef, WirePage};

    pub fn serialize<T: WirePage, S: Serializer>(page: &T, s: S) -> Result<S::Ok, S::Error> {
        page.to_page_ref().serialize(s)
    }

    pub fn deserialize<'de, T: WirePage, D: Deserializer<'de>>(d: D) -> Result<T, D::Error> {
        Ok(T::from_page_ref(PageRef::deserialize(d)?))
    }
}

/// `#[serde(with)]` for a list of pages the wire spells as `PageRef[]`.
pub mod page_refs {
    use serde::{Deserialize, Deserializer, Serializer};

    use super::{PageRef, WirePage};

    pub fn serialize<T: WirePage, S: Serializer>(pages: &[T], s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(pages.iter().map(|page| page.to_page_ref()))
    }

    pub fn deserialize<'de, T: WirePage, D: Deserializer<'de>>(d: D) -> Result<Vec<T>, D::Error> {
        Ok(Vec::<PageRef>::deserialize(d)?.into_iter().map(T::from_page_ref).collect())
    }
}

/// `#[serde(with)]` for a page that may be absent: `PageRef | null`.
pub mod optional_page_ref {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use super::{PageRef, WirePage};

    pub fn serialize<T: WirePage, S: Serializer>(
        page: &Option<T>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        page.map(T::to_page_ref).serialize(s)
    }

    pub fn deserialize<'de, T: WirePage, D: Deserializer<'de>>(
        d: D,
    ) -> Result<Option<T>, D::Error> {
        Ok(Option::<PageRef>::deserialize(d)?.map(T::from_page_ref))
    }
}

/// The two durable ways to address an annotation across reads.
///
/// `ObjectNumber` is `EPDFAnnot_GetObjectNumber`, `> 0` for indirect objects,
/// which is the overwhelming common case. `Nm` is the annotation's `/NM`
/// entry. Reads never write `/NM`: a client can opt into symbolic ids by
/// passing one to create, but a read never mutates the document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AnnotationStableId {
    #[serde(rename = "objectNumber")]
    ObjectNumber { value: u32 },
    #[serde(rename = "nm")]
    Nm { value: String },
}

impl AnnotationStableId {
    /// URL-safe encoding used as the `:annotKey` route parameter by the cloud
    /// HTTP surface: `obj:42`, or `nm:` followed by the opaque `/NM` string.
    ///
    /// The caller percent-encodes the result before splicing it into a path.
    /// The `nm:` prefix is what lets the decoder tell an arbitrary `/NM` from
    /// a numeric id unambiguously.
    pub fn encode_key(&self) -> Result<String, EngineError> {
        match self {
            Self::ObjectNumber { value } => {
                if *value == 0 {
                    return Err(EngineError::with_message(
                        EngineErrorCode::InvalidArg,
                        "objectNumber must be a positive integer, got 0",
                    ));
                }
                Ok(format!("obj:{value}"))
            }
            Self::Nm { value } => Ok(format!("nm:{value}")),
        }
    }

    /// Inverse of [`Self::encode_key`], over an already percent-decoded
    /// segment. `None` for malformed input, so a server can answer
    /// `InvalidArg` with a useful message rather than a panic.
    pub fn decode_key(key: &str) -> Option<Self> {
        if let Some(rest) = key.strip_prefix("obj:") {
            let n: u32 = rest.parse().ok()?;
            // Reject a value whose canonical spelling differs, so "obj:007"
            // and "obj:+7" do not both address annotation 7.
            if n == 0 || n.to_string() != rest {
                return None;
            }
            return Some(Self::ObjectNumber { value: n });
        }
        if let Some(value) = key.strip_prefix("nm:") {
            // An empty /NM is not an identity.
            if value.is_empty() {
                return None;
            }
            return Some(Self::Nm { value: value.to_string() });
        }
        None
    }
}

/// Whether a client can hold a long-lived reference to an annotation.
///
/// `Durable` means it has a stable wire id and references survive structural
/// mutations. `Weak` means the only handle is
/// `(pageObjectNumber, index, revision)`, and the index can shift after any
/// structural mutation on the same page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnnotationIdentityQuality {
    Durable,
    Weak,
}

/// How an annotation is addressed: `AnnotationRef` in
/// `core/src/identity/AnnotationRef.ts`.
///
/// On the wire a request names an annotation two ways (bc0057f3a, 595bdc6c7):
/// `{kind: "objectNumber", page, objectNumber}` for one born as a PDF object,
/// and `{kind: "baseIndex", page, baseIndex}` for one born inline in the
/// uploaded file, named by its position in that page's `/Annots` there. The
/// `nm` and `index` kinds are gone from the wire: [`Deserialize`] refuses
/// them.
///
/// [`Nm`](Self::Nm) and [`Index`](Self::Index) stay in the enum for the
/// engine's own callers until their owners retire them: the annotation
/// readers still answer them (Task 8.12) and the weak-annotation bookkeeping
/// still resolves them (Task 8.16). Neither can arrive in a request. An
/// `Index` must carry a fresh [`RevisionToken`], checked against the page's
/// generation counter on every dereference; a stale token fails with
/// [`EngineErrorCode::InvalidReference`].
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AnnotationRef {
    #[serde(rename = "objectNumber")]
    ObjectNumber {
        #[serde(with = "page_ref")]
        page: PageObjectNumber,
        #[serde(rename = "objectNumber")]
        annot: u32,
    },
    #[serde(rename = "baseIndex")]
    BaseIndex {
        #[serde(with = "page_ref")]
        page: PageObjectNumber,
        #[serde(rename = "baseIndex")]
        base_index: u32,
    },
    #[serde(rename = "nm")]
    Nm {
        #[serde(with = "page_ref")]
        page: PageObjectNumber,
        nm: String,
    },
    #[serde(rename = "index")]
    Index {
        #[serde(with = "page_ref")]
        page: PageObjectNumber,
        index: u32,
        revision: RevisionToken,
    },
}

/// Only the two kinds the wire has. A request naming an annotation by `nm`
/// or `index` is refused as an unknown variant rather than resolved.
impl<'de> Deserialize<'de> for AnnotationRef {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(tag = "kind")]
        enum Wire {
            #[serde(rename = "objectNumber", rename_all = "camelCase")]
            ObjectNumber {
                #[serde(with = "page_ref")]
                page: PageObjectNumber,
                object_number: u32,
            },
            #[serde(rename = "baseIndex", rename_all = "camelCase")]
            BaseIndex {
                #[serde(with = "page_ref")]
                page: PageObjectNumber,
                base_index: u32,
            },
        }
        Ok(match Wire::deserialize(d)? {
            Wire::ObjectNumber { page, object_number } => {
                Self::ObjectNumber { page, annot: object_number }
            }
            Wire::BaseIndex { page, base_index } => Self::BaseIndex { page, base_index },
        })
    }
}

impl AnnotationRef {
    pub fn page(&self) -> PageObjectNumber {
        match self {
            Self::ObjectNumber { page, .. }
            | Self::BaseIndex { page, .. }
            | Self::Nm { page, .. }
            | Self::Index { page, .. } => *page,
        }
    }

    /// A reference by index is weak by construction; the others are
    /// durable. A `baseIndex` is a name for life (`AnnotationRef.ts`), not a
    /// position that shifts.
    pub fn quality(&self) -> AnnotationIdentityQuality {
        match self {
            Self::Index { .. } => AnnotationIdentityQuality::Weak,
            _ => AnnotationIdentityQuality::Durable,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_direct_page_object_is_a_malformed_pdf_not_a_page() {
        let err = PageObjectNumber::new(0).unwrap_err();
        assert!(err.is(EngineErrorCode::MalformedPdf));
        assert_eq!(PageObjectNumber::new(7).unwrap().get(), 7);
    }

    #[test]
    fn stable_id_keys_round_trip() {
        let obj = AnnotationStableId::ObjectNumber { value: 42 };
        assert_eq!(obj.encode_key().unwrap(), "obj:42");
        assert_eq!(AnnotationStableId::decode_key("obj:42"), Some(obj));

        let nm = AnnotationStableId::Nm { value: "foo bar".into() };
        assert_eq!(nm.encode_key().unwrap(), "nm:foo bar");
        assert_eq!(AnnotationStableId::decode_key("nm:foo bar"), Some(nm));
    }

    #[test]
    fn stable_id_keys_reject_what_would_alias() {
        // Two spellings of one id would let a client cache the same annotation
        // under two keys.
        assert_eq!(AnnotationStableId::decode_key("obj:007"), None);
        assert_eq!(AnnotationStableId::decode_key("obj:+7"), None);
        assert_eq!(AnnotationStableId::decode_key("obj:0"), None);
        assert_eq!(AnnotationStableId::decode_key("obj:-1"), None);
        assert_eq!(AnnotationStableId::decode_key("nm:"), None);
        assert_eq!(AnnotationStableId::decode_key("42"), None);
        assert!(AnnotationStableId::ObjectNumber { value: 0 }.encode_key().is_err());
    }

    #[test]
    fn an_nm_containing_the_other_prefix_still_decodes_as_an_nm() {
        // /NM values are opaque strings and may contain anything.
        let id = AnnotationStableId::Nm { value: "obj:9".into() };
        assert_eq!(AnnotationStableId::decode_key(&id.encode_key().unwrap()), Some(id));
    }

    #[test]
    fn only_index_references_are_weak() {
        let page = PageObjectNumber::new(3).unwrap();
        assert_eq!(
            AnnotationRef::ObjectNumber { page, annot: 9 }.quality(),
            AnnotationIdentityQuality::Durable
        );
        assert_eq!(
            AnnotationRef::Nm { page, nm: "a".into() }.quality(),
            AnnotationIdentityQuality::Durable
        );
        let weak = AnnotationRef::Index {
            page,
            index: 0,
            revision: RevisionToken::new("session-1", page, 4),
        };
        assert_eq!(weak.quality(), AnnotationIdentityQuality::Weak);
        assert_eq!(weak.page(), page);
    }

    #[test]
    fn refs_keep_the_typescript_wire_field_names() {
        let page = PageObjectNumber::new(3).unwrap();
        assert_eq!(
            serde_json::to_value(AnnotationRef::ObjectNumber { page, annot: 9 }).unwrap(),
            serde_json::json!({
                "kind": "objectNumber",
                "page": { "kind": "objectNumber", "objectNumber": 3 },
                "objectNumber": 9,
            })
        );
    }

    #[test]
    fn a_page_ref_is_the_one_tagged_kind() {
        let page = PageObjectNumber::new(12).unwrap();
        let wire = serde_json::json!({ "kind": "objectNumber", "objectNumber": 12 });
        assert_eq!(serde_json::to_value(PageRef::new(page)).unwrap(), wire);
        assert_eq!(PageRef::from_value(&wire, "page").unwrap().object_number(), page);
    }

    #[test]
    fn a_malformed_page_ref_is_an_invalid_argument() {
        // `PageRefSchema` refuses all of these; a dispatcher that read the
        // number out of an unknown kind would address a page the caller never
        // named.
        for bad in [
            serde_json::json!({ "kind": "index", "objectNumber": 1 }),
            serde_json::json!({ "kind": "objectNumber" }),
            serde_json::json!({ "kind": "objectNumber", "objectNumber": "3" }),
            serde_json::json!({ "kind": "objectNumber", "objectNumber": 0 }),
            serde_json::json!({ "kind": "objectNumber", "objectNumber": -3 }),
            serde_json::json!({ "kind": "objectNumber", "objectNumber": 1.5 }),
            serde_json::json!({ "kind": "objectNumber", "pageObjectNumber": 3 }),
            serde_json::json!({ "objectNumber": 3 }),
            serde_json::json!(3),
        ] {
            let err = PageRef::from_value(&bad, "page").unwrap_err();
            assert!(err.is(EngineErrorCode::InvalidArg), "{bad} answered {err:?}");
        }
    }

    #[test]
    fn an_annotation_ref_reads_the_two_wire_kinds() {
        let page = PageObjectNumber::new(3).unwrap();
        let by_number = serde_json::json!({ "kind": "objectNumber",
            "page": { "kind": "objectNumber", "objectNumber": 3 }, "objectNumber": 9 });
        assert_eq!(
            serde_json::from_value::<AnnotationRef>(by_number.clone()).unwrap(),
            AnnotationRef::ObjectNumber { page, annot: 9 }
        );
        let by_base = serde_json::json!({ "kind": "baseIndex",
            "page": { "kind": "objectNumber", "objectNumber": 3 }, "baseIndex": 2 });
        let base = serde_json::from_value::<AnnotationRef>(by_base.clone()).unwrap();
        assert_eq!(base, AnnotationRef::BaseIndex { page, base_index: 2 });
        assert_eq!(base.quality(), AnnotationIdentityQuality::Durable);
        assert_eq!(serde_json::to_value(&base).unwrap(), by_base);
    }

    #[test]
    fn the_nm_and_index_kinds_are_gone_from_the_wire() {
        // bc0057f3a: an annotation's `/NM` is data, not a name, and a
        // position is not an identity. A request that still names one is
        // refused rather than resolved.
        let page = serde_json::json!({ "kind": "objectNumber", "objectNumber": 3 });
        for gone in [
            serde_json::json!({ "kind": "nm", "page": page, "nm": "a" }),
            serde_json::json!({ "kind": "index", "page": page, "index": 0,
                "revision": { "docSessionId": "s", "page": page, "generation": 0 } }),
            serde_json::json!({ "kind": "objectNumber", "page": page, "annotObjectNumber": 9 }),
        ] {
            assert!(serde_json::from_value::<AnnotationRef>(gone.clone()).is_err(), "{gone}");
        }
    }
}
