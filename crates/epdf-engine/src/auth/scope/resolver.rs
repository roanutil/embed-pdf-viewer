//! What a scope array actually grants.
//!
//! A transcription of `packages/engine/core/src/auth/scope/resolver.ts`. The
//! reasoning behind the narrowing model and the bit expansion table lives
//! there and is not repeated; what is repeated is anything Rust does
//! differently.

use std::collections::BTreeSet;

use super::parser::{InvalidScope, parse_scope};
use super::types::{
    CollabAction, CollabEntity, CollabFilter, CollabTarget, DocCapability, FieldAction,
    FieldFilter, FieldGroupRight, FieldRights, FieldWriteAction, IdentityClaims, ParsedScope,
    PdfBits,
};

/// The granted set.
///
/// A `BTreeSet` rather than a `HashSet` so iteration is ordered and a caller
/// that serializes it, such as the `/access` response's `effectiveScope`,
/// gets a stable answer. TypeScript returns an insertion-ordered `Set` and
/// its callers sort; sorting by construction costs nothing here.
pub type Capabilities = BTreeSet<DocCapability>;

/// Parse every entry, failing on the first bad one.
///
/// TypeScript's resolver calls `rawScope.map(parseScope)` inside each check
/// and lets `InvalidScope` propagate. Rust surfaces it as a `Result` instead
/// of panicking, so the checks below take already-parsed scopes and the
/// caller decides once what an unparseable scope means. A caller that treats
/// the error as "deny" gets TypeScript's behaviour at a route guard; one that
/// reports it gets TypeScript's behaviour at verify time.
pub fn parse_all(raw: &[String]) -> Result<Vec<ParsedScope>, InvalidScope> {
    raw.iter().map(|s| parse_scope(s)).collect()
}

fn has_wildcard(parsed: &[ParsedScope]) -> bool {
    parsed.iter().any(|s| matches!(s, ParsedScope::Wildcard))
}

/// True iff the capability is granted.
///
/// The wildcard short-circuits before expansion, which is why this is not
/// just a membership test on [`expanded_capabilities`].
pub fn check_capability(
    capability: DocCapability,
    parsed: &[ParsedScope],
    pdf_bits: PdfBits,
) -> bool {
    if has_wildcard(parsed) {
        return true;
    }
    expanded_capabilities(parsed, pdf_bits).contains(&capability)
}

/// True iff at least one of `capabilities` is granted.
pub fn check_any_capability(
    capabilities: &[DocCapability],
    parsed: &[ParsedScope],
    pdf_bits: PdfBits,
) -> bool {
    capabilities.iter().any(|c| check_capability(*c, parsed, pdf_bits))
}

/// True iff the scope grants the collab action against the target record.
///
/// The narrowing model, in the order `resolver.ts` states it: wildcard wins;
/// otherwise any collab scope applicable to this action shadows the broad
/// `doc.annotate.modify` default and only those filters decide; otherwise
/// `doc.annotate.modify` allows; otherwise deny.
///
/// That middle clause is the whole point. `[doc.annotate.modify,
/// annotations:update:self]` means "edit your own", and an implementation
/// that checked `modify` first would turn it into "edit anyone".
pub fn check_collab(
    action: CollabAction,
    target: &CollabTarget,
    parsed: &[ParsedScope],
    identity: &IdentityClaims,
    pdf_bits: PdfBits,
) -> bool {
    if has_wildcard(parsed) {
        return true;
    }

    let mut applicable = parsed.iter().filter_map(|s| match s {
        ParsedScope::Collab { action: pattern, filter, .. } if pattern.covers(action) => {
            Some(filter)
        }
        _ => None,
    });

    if let Some(first) = applicable.next() {
        return std::iter::once(first)
            .chain(applicable)
            .any(|filter| filter_matches(filter, target, identity));
    }

    expanded_capabilities(parsed, pdf_bits).contains(&DocCapability::DocAnnotateModify)
}

/// The full granted set after expanding `pdf.permissions` and applying the
/// implication rules.
///
/// Does not short-circuit on the wildcard; callers do that themselves.
pub fn expanded_capabilities(parsed: &[ParsedScope], pdf_bits: PdfBits) -> Capabilities {
    let mut out = Capabilities::new();
    let mut has_annotation_collab = false;
    let mut has_field_collab = false;

    for scope in parsed {
        match scope {
            ParsedScope::Capability(name) => {
                out.insert(*name);
            }
            ParsedScope::Virtual => out.extend(materialize_pdf_permissions(pdf_bits)),
            ParsedScope::Collab { .. } => has_annotation_collab = true,
            ParsedScope::FieldCollab { .. } => has_field_collab = true,
            ParsedScope::Wildcard => {}
        }
    }

    // Implications, after the explicit additions. The order of the two forms
    // rules is load-bearing: `forms.modify` adds `forms.fill`, and the
    // `forms.fill` rule below then has to see it. Swap them and a
    // `doc.forms.modify`-only scope loses `doc.forms.read`, which
    // `scope_conformance.rs` catches.
    //
    // Given that, the `forms.read` insert in the modify branch is redundant.
    // It is kept because the TypeScript has it, and a transcription that
    // silently drops a line is harder to audit against its original than one
    // that carries a line it does not need.
    if out.contains(&DocCapability::DocAnnotateModify) {
        out.insert(DocCapability::DocAnnotateRead);
    }
    if out.contains(&DocCapability::DocFormsModify) {
        out.insert(DocCapability::DocFormsFill);
        out.insert(DocCapability::DocFormsRead);
    }
    if out.contains(&DocCapability::DocFormsFill) {
        out.insert(DocCapability::DocFormsRead);
    }
    if has_annotation_collab {
        out.insert(DocCapability::DocAnnotateRead);
    }
    // Filling in or signing a field means seeing the form: groups limit
    // writes, never reads.
    if has_field_collab {
        out.insert(DocCapability::DocFormsRead);
    }

    out
}

/// Parse and expand in one step.
pub fn expand_raw_scope(raw: &[String], pdf_bits: PdfBits) -> Result<Capabilities, InvalidScope> {
    Ok(expanded_capabilities(&parse_all(raw)?, pdf_bits))
}

/// Authority to put a record of `entity` in a specific group: an
/// annotation, or a form field (`checkSetGroup`).
///
/// Independent of the write capabilities and of group membership, both
/// deliberately: those describe row access and map to PDF bits, while this
/// is a cloud-only assignment authority with no bit behind it, and
/// `set-group:group=legal` is meant to work for someone who is not in legal.
/// Each entity's set-group is its own: an `annotations:` scope puts no field
/// in a group, and a `fields:` scope no annotation.
pub fn check_set_group(
    entity: CollabEntity,
    new_group_id: &str,
    caller_default_group_id: Option<&str>,
    parsed: &[ParsedScope],
) -> bool {
    // Assigning your own default group is not a reassignment.
    if Some(new_group_id) == caller_default_group_id {
        return true;
    }
    if has_wildcard(parsed) {
        return true;
    }

    for scope in parsed {
        match (entity, scope) {
            (CollabEntity::Annotations, ParsedScope::Collab { action, filter, .. }) => {
                if !action.covers(CollabAction::SetGroup) {
                    continue;
                }
                match filter {
                    CollabFilter::All => return true,
                    CollabFilter::Group(id) if id == new_group_id => return true,
                    // `:self` and `:createdBy` are refused at parse time for
                    // an explicit set-group, so these only arrive through an
                    // action wildcard that is legitimate for other actions.
                    // They cannot satisfy a set-group check, so skip rather
                    // than deny: a later scope may still grant it.
                    _ => {}
                }
            }
            (CollabEntity::Fields, ParsedScope::FieldCollab { action, filter }) => {
                if !action.covers(FieldAction::SetGroup) {
                    continue;
                }
                match filter {
                    FieldFilter::All => return true,
                    FieldFilter::Group(id) if id == new_group_id => return true,
                    FieldFilter::Group(_) => {}
                }
            }
            _ => {}
        }
    }
    false
}

/// The field scopes that name `action`, explicitly or by the wildcard.
fn field_filters(parsed: &[ParsedScope], action: FieldAction) -> Vec<&FieldFilter> {
    parsed
        .iter()
        .filter_map(|s| match s {
            ParsedScope::FieldCollab { action: pattern, filter } if pattern.covers(action) => {
                Some(filter)
            }
            _ => None,
        })
        .collect()
}

/// True iff the scope lets the caller fill in or sign a form field in
/// `group_id` (`None` for a field in no group) (`checkFieldAction`).
///
/// Narrowing, per action, as for annotations: the wildcard allows; if any
/// `fields:<action>` scope (or `fields:*`) exists, only those decide, and a
/// field in no group then matches only `:all`; otherwise the broad
/// capability decides for every field. Nothing here narrows form design
/// (`doc.forms.modify`).
///
/// No `protection` argument, as [`check_capability`] has none: the session
/// authority removes what the signatures forbid.
pub fn check_field_action(
    action: FieldWriteAction,
    group_id: Option<&str>,
    parsed: &[ParsedScope],
    pdf_bits: PdfBits,
) -> bool {
    if has_wildcard(parsed) {
        return true;
    }
    let applicable = field_filters(parsed, action.action());
    if !applicable.is_empty() {
        return applicable.iter().any(|filter| match filter {
            FieldFilter::All => true,
            FieldFilter::Group(id) => group_id == Some(id.as_str()),
        });
    }
    expanded_capabilities(parsed, pdf_bits).contains(&action.broad())
}

/// Whether the scope lets the caller fill in or sign some form field: the
/// broad capability, or any `fields:<action>` scope (`checkAnyFieldAction`).
/// What a form verb checks before its fields are known.
pub fn check_any_field_action(
    action: FieldWriteAction,
    parsed: &[ParsedScope],
    pdf_bits: PdfBits,
) -> bool {
    if has_wildcard(parsed) {
        return true;
    }
    if !field_filters(parsed, action.action()).is_empty() {
        return true;
    }
    expanded_capabilities(parsed, pdf_bits).contains(&action.broad())
}

/// [`check_field_action`] and [`check_set_group`] for fields as group lists
/// (`fieldRightsOf`): for each action, every field or the groups the scope
/// names, so a designer can offer exactly the groups a field may go in.
pub fn field_rights_of(
    parsed: &[ParsedScope],
    pdf_bits: PdfBits,
    caller_default_group_id: Option<&str>,
) -> FieldRights {
    let wildcard = has_wildcard(parsed);
    // Groups in the order the scopes name them, each once, then `extra`.
    let from_scopes = |filters: Vec<&FieldFilter>, extra: Option<&str>| -> FieldGroupRight {
        if filters.iter().any(|f| matches!(f, FieldFilter::All)) {
            return FieldGroupRight::All;
        }
        let mut groups: Vec<String> = Vec::new();
        let named = filters.iter().filter_map(|f| match f {
            FieldFilter::Group(id) => Some(id.as_str()),
            FieldFilter::All => None,
        });
        for group in named.chain(extra) {
            if !groups.iter().any(|g| g == group) {
                groups.push(group.to_string());
            }
        }
        FieldGroupRight::Groups(groups)
    };
    let writing = |action: FieldWriteAction| -> FieldGroupRight {
        if wildcard {
            return FieldGroupRight::All;
        }
        let filters = field_filters(parsed, action.action());
        if !filters.is_empty() {
            return from_scopes(filters, None);
        }
        if expanded_capabilities(parsed, pdf_bits).contains(&action.broad()) {
            FieldGroupRight::All
        } else {
            FieldGroupRight::none()
        }
    };
    FieldRights {
        fill: writing(FieldWriteAction::Fill),
        sign: writing(FieldWriteAction::Sign),
        set_group: if wildcard {
            FieldGroupRight::All
        } else {
            from_scopes(field_filters(parsed, FieldAction::SetGroup), caller_default_group_id)
        },
    }
}

/// Test one collab filter against a target and the caller's identity.
///
/// Every filter but `self` compares one fact on the record with the value
/// the scope names. `group=X` matches a record in group X whatever groups
/// the caller belongs to (upstream 4b2cdb395 dropped the membership test):
/// the scope is what says the caller may act on X's records.
pub fn filter_matches(
    filter: &CollabFilter,
    target: &CollabTarget,
    identity: &IdentityClaims,
) -> bool {
    match filter {
        CollabFilter::All => true,
        CollabFilter::SelfOnly => identity.user_id.is_some() && target.user_id == identity.user_id,
        CollabFilter::CreatedBy(user_id) => target.user_id.as_deref() == Some(user_id.as_str()),
        CollabFilter::Group(group_id) => target.group_id.as_deref() == Some(group_id.as_str()),
    }
}

/// The capabilities `pdf.permissions` stands for under these bits.
///
/// `materializePdfPermissions` (`builders.ts`), the one reading of the bits:
/// the resolver and the permission advisory
/// ([`crate::features::security::state::advisory_from_pdf_bits`]) both use
/// it. It follows what Acrobat allows on a file with these bits, which is
/// broader than ISO 32000 in two places: bit 4 also allows filling in and
/// signing, and bit 5 governs taking the text out, not finding or selecting
/// it.
pub fn materialize_pdf_permissions(b: PdfBits) -> Capabilities {
    let mut out = Capabilities::new();

    // A working session, and reading what is there: bit 6 governs writing
    // annotations and form values, not seeing them, and bit 5 taking text
    // out, not finding or selecting it.
    out.insert(DocCapability::DocOpen);
    out.insert(DocCapability::DocRender);
    out.insert(DocCapability::DocAnnotateRead);
    out.insert(DocCapability::DocFormsRead);
    out.insert(DocCapability::DocTextSearch);
    out.insert(DocCapability::DocTextSelect);

    if b.bit5 {
        out.insert(DocCapability::DocTextCopy);
        out.insert(DocCapability::DocContentCopy);
    }
    if b.bit3 {
        out.insert(DocCapability::DocPrint);
    }
    // Bit 12 refines bit 3 rather than standing alone, so high-quality print
    // without print is not a grant.
    if b.bit12 && b.bit3 {
        out.insert(DocCapability::DocPrintHigh);
    }
    if b.bit4 {
        out.insert(DocCapability::DocPagesModify);
        out.insert(DocCapability::DocRedact);
        out.insert(DocCapability::DocMetadataModify);
        out.insert(DocCapability::DocAttachmentsModify);
    }
    // Bit 11 assembles pages and creates outline items even when bit 4 is
    // clear; bit 4 includes it.
    if b.bit4 || b.bit11 {
        out.insert(DocCapability::DocPagesAssemble);
        out.insert(DocCapability::DocOutlineModify);
    }
    if b.bit6 {
        out.insert(DocCapability::DocAnnotateModify);
    }
    // Signing a signature field is filling it in, and any of the three bits
    // lets Acrobat (and PDFium's form filler) fill in.
    if b.bit4 || b.bit6 || b.bit9 {
        out.insert(DocCapability::DocFormsFill);
        out.insert(DocCapability::DocSign);
    }
    // Changing existing fields takes both.
    if b.bit6 && b.bit4 {
        out.insert(DocCapability::DocFormsModify);
    }
    // No bit expands to doc.forms.submit, doc.forms.import, doc.forms.script,
    // doc.annotate.import, doc.download, doc.download.flattened or
    // doc.sign.certify: they are grant-minted or cloud-only, so they are
    // granted explicitly or not held at all.
    out
}
