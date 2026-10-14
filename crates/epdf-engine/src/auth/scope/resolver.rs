//! What a scope array actually grants.
//!
//! A transcription of `packages/engine/core/src/auth/scope/resolver.ts`. The
//! reasoning behind the narrowing model and the bit expansion table lives
//! there and is not repeated; what is repeated is anything Rust does
//! differently.

use std::collections::BTreeSet;

use super::parser::{InvalidScope, parse_scope};
use super::types::{
    CollabAction, CollabFilter, CollabTarget, DocCapability, IdentityClaims, ParsedScope, PdfBits,
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

    for scope in parsed {
        match scope {
            ParsedScope::Capability(name) => {
                out.insert(*name);
            }
            ParsedScope::Virtual => add_pdf_permissions(&mut out, pdf_bits),
            ParsedScope::Collab { .. } => has_annotation_collab = true,
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

    out
}

/// Parse and expand in one step.
pub fn expand_raw_scope(raw: &[String], pdf_bits: PdfBits) -> Result<Capabilities, InvalidScope> {
    Ok(expanded_capabilities(&parse_all(raw)?, pdf_bits))
}

/// Authority to assign a specific group to an annotation.
///
/// Independent of `doc.annotate.modify` and of group membership, both
/// deliberately: `modify` describes row access and maps to a PDF bit, while
/// this is a cloud-only assignment authority with no bit behind it, and
/// `set-group:group=legal` is meant to work for someone who is not in legal.
pub fn check_set_group(
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
        let ParsedScope::Collab { action, filter, .. } = scope else {
            continue;
        };
        if !action.covers(CollabAction::SetGroup) {
            continue;
        }
        match filter {
            CollabFilter::All => return true,
            CollabFilter::Group(id) if id == new_group_id => return true,
            // `:self` and `:createdBy` are refused at parse time for an
            // explicit set-group, so these only arrive through an action
            // wildcard that is legitimate for other actions. They cannot
            // satisfy a set-group check, so skip rather than deny: a later
            // scope in the array may still grant it.
            _ => {}
        }
    }
    false
}

/// Test one collab filter against a target and the caller's identity.
///
/// `group=X` checks membership as well as the row's group, which is what
/// stops a token matching annotations in a group it does not belong to even
/// when the row carries that group.
pub fn filter_matches(
    filter: &CollabFilter,
    target: &CollabTarget,
    identity: &IdentityClaims,
) -> bool {
    match filter {
        CollabFilter::All => true,
        CollabFilter::SelfOnly => identity.user_id.is_some() && target.user_id == identity.user_id,
        CollabFilter::CreatedBy(user_id) => target.user_id.as_deref() == Some(user_id.as_str()),
        CollabFilter::Group(group_id) => {
            target.group_id.as_deref() == Some(group_id.as_str())
                && identity.groups.iter().any(|g| g == group_id)
        }
    }
}

/// Translate `pdf.permissions` into concrete capabilities under these bits.
///
/// `doc.open` and `doc.render` are unconditional because the virtual scope
/// means "give the user a working session", and reads of annotations and form
/// values are unconditional because ISO 32000 bit 6 governs writing and not
/// visibility.
fn add_pdf_permissions(out: &mut Capabilities, b: PdfBits) {
    out.insert(DocCapability::DocOpen);
    out.insert(DocCapability::DocRender);
    out.insert(DocCapability::DocAnnotateRead);
    out.insert(DocCapability::DocFormsRead);

    if b.bit5 {
        out.insert(DocCapability::DocTextSelect);
        out.insert(DocCapability::DocTextCopy);
        out.insert(DocCapability::DocTextSearch);
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
    if b.bit11 {
        out.insert(DocCapability::DocPagesAssemble);
    }
    if b.bit6 {
        out.insert(DocCapability::DocAnnotateModify);
    }
    if b.bit6 || b.bit9 {
        out.insert(DocCapability::DocFormsFill);
        out.insert(DocCapability::DocSign);
    }
    if b.bit6 && b.bit4 {
        out.insert(DocCapability::DocFormsModify);
    }
    // No bit expands to doc.forms.submit, doc.download,
    // doc.download.flattened or doc.sign.certify. ISO 32000 defines no
    // submit permission, the download capabilities are cloud-only, and a
    // certification is grant-minted only, so they are granted explicitly or
    // not held at all.
}
