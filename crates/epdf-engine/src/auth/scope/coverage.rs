//! Minting only what you hold.
//!
//! A transcription of `packages/engine/core/src/auth/scope/coverage.ts`. The
//! per-entry rules are stated there; what is repeated here is anything the
//! Rust shape changes.

use std::collections::{BTreeMap, BTreeSet};

use super::resolver::{Capabilities, expanded_capabilities};
use super::types::{
    CollabAction, CollabFilter, DocCapability, FieldAction, FieldFilter, ParsedScope, PdfBits,
};

/// What `wanted` would allow that `held` does not, in scope words, sorted;
/// empty when `held` covers it (`scopeNotCovered`).
///
/// `held` covers `wanted` when, on every document, everything `wanted`
/// allows `held` allows too. `pdf.permissions` reads the file's permission
/// bits, so when either side has it both are compared under every
/// combination of them.
///
/// Takes parsed scopes, as the resolver's checks do, where TypeScript takes
/// the raw arrays and lets a parse error throw.
pub fn scope_not_covered(wanted: &[ParsedScope], held: &[ParsedScope]) -> Vec<String> {
    if held.iter().any(|s| matches!(s, ParsedScope::Wildcard)) {
        return Vec::new();
    }
    if wanted.iter().any(|s| matches!(s, ParsedScope::Wildcard)) {
        return vec!["*".to_string()];
    }

    let reads_the_file = held.iter().chain(wanted).any(|s| matches!(s, ParsedScope::Virtual));
    let every_bits: Vec<PdfBits> =
        if reads_the_file { every_pdf_bits().collect() } else { vec![PdfBits::default()] };

    let mut beyond = BTreeSet::new();
    for bits in every_bits {
        let mine = authority_of(held, bits);
        let theirs = authority_of(wanted, bits);
        for capability in &theirs.capabilities {
            if !mine.capabilities.contains(capability) {
                beyond.insert(capability.as_str().to_string());
            }
        }
        for (write, filters) in &theirs.writes {
            let mine_filters = mine.writes.get(write).map(Vec::as_slice).unwrap_or_default();
            for filter in filters {
                if !mine_filters.iter().any(|m| filter_covers(m, filter, write)) {
                    beyond.insert(format!("{write}:{}", filter_word(filter)));
                }
            }
        }
        for permission in &theirs.plugins {
            if !mine.plugins.contains(permission) {
                beyond.insert(permission.clone());
            }
        }
    }

    // `[...beyond].sort()`: JavaScript compares UTF-16 code units, which a
    // group id outside the Basic Multilingual Plane would order differently
    // from Rust's byte order.
    let mut out: Vec<String> = beyond.into_iter().collect();
    out.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
    out
}

/// What a scope allows on a file with these bits, in the three shapes
/// [`scope_not_covered`] compares.
struct Authority {
    capabilities: Capabilities,
    /// Per collab write (`annotations:update`), the records it reaches.
    writes: BTreeMap<&'static str, Vec<CollabFilter>>,
    plugins: BTreeSet<String>,
}

/// One collab write (`WRITES`), with the capability that allows it on every
/// record when no collab scope narrows it. Putting a record in a group has
/// none: only a `set-group` scope (or `*`) allows it.
#[derive(Clone, Copy)]
enum Write {
    Annotation(CollabAction),
    Field(FieldAction),
}

const WRITES: [(&str, Write, Option<DocCapability>); 7] = [
    (
        "annotations:create",
        Write::Annotation(CollabAction::Create),
        Some(DocCapability::DocAnnotateModify),
    ),
    (
        "annotations:update",
        Write::Annotation(CollabAction::Update),
        Some(DocCapability::DocAnnotateModify),
    ),
    (
        "annotations:delete",
        Write::Annotation(CollabAction::Delete),
        Some(DocCapability::DocAnnotateModify),
    ),
    ("annotations:set-group", Write::Annotation(CollabAction::SetGroup), None),
    ("fields:fill", Write::Field(FieldAction::Fill), Some(DocCapability::DocFormsFill)),
    ("fields:sign", Write::Field(FieldAction::Sign), Some(DocCapability::DocSign)),
    ("fields:set-group", Write::Field(FieldAction::SetGroup), None),
];

fn authority_of(parsed: &[ParsedScope], bits: PdfBits) -> Authority {
    let capabilities = expanded_capabilities(parsed, bits);
    let mut writes = BTreeMap::new();
    for (key, write, broad) in WRITES {
        // The same narrowing as `check_collab` and `check_field_action`: a
        // collab scope for the action decides alone, else the broad
        // capability.
        let narrowing: Vec<CollabFilter> = parsed
            .iter()
            .filter_map(|s| match (write, s) {
                (
                    Write::Annotation(action),
                    ParsedScope::Collab { action: pattern, filter, .. },
                ) if pattern.covers(action) => Some(filter.clone()),
                (Write::Field(action), ParsedScope::FieldCollab { action: pattern, filter })
                    if pattern.covers(action) =>
                {
                    Some(match filter {
                        FieldFilter::All => CollabFilter::All,
                        FieldFilter::Group(group) => CollabFilter::Group(group.clone()),
                    })
                }
                _ => None,
            })
            .collect();
        let is_set_group = matches!(
            write,
            Write::Annotation(CollabAction::SetGroup) | Write::Field(FieldAction::SetGroup)
        );
        // `:self` and `createdBy=` mean nothing to set-group (`check_set_group`
        // skips them).
        let filters = if is_set_group {
            narrowing
                .into_iter()
                .filter(|f| matches!(f, CollabFilter::All | CollabFilter::Group(_)))
                .collect()
        } else if !narrowing.is_empty() {
            narrowing
        } else if broad.is_some_and(|b| capabilities.contains(&b)) {
            vec![CollabFilter::All]
        } else {
            Vec::new()
        };
        if !filters.is_empty() {
            writes.insert(key, filters);
        }
    }
    let plugins = parsed
        .iter()
        .filter_map(|s| match s {
            ParsedScope::Plugin { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    Authority { capabilities, writes, plugins }
}

fn filter_covers(mine: &CollabFilter, theirs: &CollabFilter, write: &str) -> bool {
    match mine {
        CollabFilter::All => true,
        CollabFilter::Group(id) => matches!(theirs, CollabFilter::Group(other) if other == id),
        CollabFilter::CreatedBy(id) => {
            matches!(theirs, CollabFilter::CreatedBy(other) if other == id)
        }
        // A create is checked against the creator, so `create:self` means the
        // same for whoever holds it.
        CollabFilter::SelfOnly => {
            matches!(theirs, CollabFilter::SelfOnly) && write == "annotations:create"
        }
    }
}

fn filter_word(filter: &CollabFilter) -> String {
    match filter {
        CollabFilter::All => "all".to_string(),
        CollabFilter::SelfOnly => "self".to_string(),
        CollabFilter::CreatedBy(user) => format!("createdBy={user}"),
        CollabFilter::Group(group) => format!("group={group}"),
    }
}

/// Every combination of the bits `pdf.permissions` reads (`EVERY_PDF_BITS`);
/// bit 10 gives nothing, so it stays clear.
fn every_pdf_bits() -> impl Iterator<Item = PdfBits> {
    (0u32..1 << 7).map(|n| PdfBits {
        bit3: n & 1 != 0,
        bit4: n & 2 != 0,
        bit5: n & 4 != 0,
        bit6: n & 8 != 0,
        bit9: n & 16 != 0,
        bit10: false,
        bit11: n & 32 != 0,
        bit12: n & 64 != 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::scope::parse_all;

    fn not_covered(wanted: &[&str], held: &[&str]) -> Vec<String> {
        let parse = |raw: &[&str]| {
            parse_all(&raw.iter().map(|s| s.to_string()).collect::<Vec<_>>()).expect("parses")
        };
        scope_not_covered(&parse(wanted), &parse(held))
    }

    #[test]
    fn a_capability_covers_the_ones_it_implies_not_the_other_way_round() {
        assert!(not_covered(&["doc.render.thumbnail"], &["doc.render"]).is_empty());
        assert_eq!(not_covered(&["doc.render"], &["doc.render.thumbnail"]), ["doc.render"]);
    }

    #[test]
    fn the_holders_narrowing_counts() {
        assert_eq!(
            not_covered(
                &["doc.annotate.modify"],
                &["doc.annotate.modify", "annotations:update:self"]
            ),
            ["annotations:update:all"],
        );
    }

    #[test]
    fn a_plugin_permission_is_covered_by_itself() {
        assert!(not_covered(&["protected-display.view"], &["protected-display.view"]).is_empty());
        assert_eq!(
            not_covered(&["protected-display.view"], &["doc.open"]),
            ["protected-display.view"]
        );
    }

    #[test]
    fn sorting_follows_utf16_code_units() {
        // By bytes U+FF5E sorts before an astral character; by UTF-16 code
        // units, JavaScript's order, the astral one's surrogate comes first.
        let got = not_covered(
            &["annotations:update:group=\u{FF5E}", "annotations:update:group=\u{1F600}"],
            &["doc.annotate.read"],
        );
        assert_eq!(
            got,
            ["annotations:update:group=\u{1F600}", "annotations:update:group=\u{FF5E}"]
        );
    }
}
