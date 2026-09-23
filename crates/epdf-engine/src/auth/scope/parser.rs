//! The scope grammar.
//!
//! A transcription of `packages/engine/core/src/auth/scope/parser.ts`.

use super::types::{
    CollabAction, CollabActionPattern, CollabEntity, CollabFilter, DocCapability, ParsedScope,
};

/// A scope string that matches no known shape.
///
/// Carries the offending string verbatim, as `InvalidScope` does, so a caller
/// can echo it. `reason` is diagnostic: the vectors compare the accept or
/// reject verdict and not this text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidScope {
    pub scope: String,
    pub reason: String,
}

impl InvalidScope {
    fn new(scope: &str, reason: impl Into<String>) -> Self {
        Self { scope: scope.to_string(), reason: reason.into() }
    }
}

impl std::fmt::Display for InvalidScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid scope \"{}\": {}", self.scope, self.reason)
    }
}

impl std::error::Error for InvalidScope {}

/// Parse one scope string.
///
/// The order of the checks is load-bearing and matches TypeScript: the
/// wildcard and the virtual scope are exact strings, anything containing a
/// colon is a collab scope even when malformed, and only what is left is
/// tried as a capability. A parser that tried capability first would report
/// "invalid capability name shape" for `annotations:create`.
pub fn parse_scope(raw: &str) -> Result<ParsedScope, InvalidScope> {
    if raw == "*" {
        return Ok(ParsedScope::Wildcard);
    }
    if raw == "pdf.permissions" {
        return Ok(ParsedScope::Virtual);
    }
    if raw.contains(':') {
        return parse_collab(raw);
    }
    parse_capability(raw)
}

/// Validate every entry, discarding the parse output.
///
/// The array stays the authoritative payload and callers re-parse at
/// resolution time, which is what TypeScript does.
pub fn validate_scope_array(raw: &[String]) -> Result<(), InvalidScope> {
    for s in raw {
        parse_scope(s)?;
    }
    Ok(())
}

fn parse_collab(raw: &str) -> Result<ParsedScope, InvalidScope> {
    // Split on the first two colons only. Filter values carry colons of their
    // own: `urn:uuid:...` group ids and `auth0|user:1` subject ids.
    //
    // Byte indices are safe here because ':' is ASCII and `find` returns a
    // char boundary, so the slices cannot split a multi-byte character.
    let Some(idx1) = raw.find(':') else {
        return Err(InvalidScope::new(raw, "collab scope requires entity:action:filter"));
    };
    let Some(offset) = raw[idx1 + 1..].find(':') else {
        return Err(InvalidScope::new(raw, "collab scope requires entity:action:filter"));
    };
    let idx2 = idx1 + 1 + offset;

    let entity = &raw[..idx1];
    let action = &raw[idx1 + 1..idx2];
    let filter_str = &raw[idx2 + 1..];

    if entity != "annotations" {
        return Err(InvalidScope::new(raw, format!("unknown collab entity: {entity}")));
    }
    let action = match action {
        "create" => CollabActionPattern::One(CollabAction::Create),
        "update" => CollabActionPattern::One(CollabAction::Update),
        "delete" => CollabActionPattern::One(CollabAction::Delete),
        "set-group" => CollabActionPattern::One(CollabAction::SetGroup),
        "*" => CollabActionPattern::Any,
        other => {
            return Err(InvalidScope::new(raw, format!("unknown collab action: {other}")));
        }
    };
    let filter = parse_filter(filter_str, raw)?;

    // `set-group` is an assignment authority over a destination group, not a
    // per-record filter. `:self` and `:createdBy` cannot be applied to "what
    // group is this annotation in", so they are refused here rather than
    // parsed into something that silently always denies.
    if action == CollabActionPattern::One(CollabAction::SetGroup)
        && matches!(filter, CollabFilter::SelfOnly | CollabFilter::CreatedBy(_))
    {
        return Err(InvalidScope::new(
            raw,
            format!("set-group only supports :all or :group=<id> filters (got :{})", filter.kind()),
        ));
    }

    Ok(ParsedScope::Collab { entity: CollabEntity::Annotations, action, filter })
}

fn parse_filter(s: &str, raw: &str) -> Result<CollabFilter, InvalidScope> {
    if s == "all" {
        return Ok(CollabFilter::All);
    }
    if s == "self" {
        return Ok(CollabFilter::SelfOnly);
    }
    if let Some(v) = s.strip_prefix("createdBy=") {
        if v.is_empty() {
            return Err(InvalidScope::new(raw, "createdBy= requires a value"));
        }
        return Ok(CollabFilter::CreatedBy(v.to_string()));
    }
    if let Some(v) = s.strip_prefix("group=") {
        if v.is_empty() {
            return Err(InvalidScope::new(raw, "group= requires a value"));
        }
        return Ok(CollabFilter::Group(v.to_string()));
    }
    Err(InvalidScope::new(raw, format!("unknown filter: {s}")))
}

/// Lowercase dotted segments, at least two of them: `^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+$`.
///
/// Written out rather than compiled, because pulling the regex crate in for
/// one anchored shape would be the only reason this module needs it, and a
/// hand-written check is easier to compare against the pattern than a
/// translated one. The `+` in the pattern is why `segments` must be at least
/// two.
fn has_capability_shape(raw: &str) -> bool {
    let mut segments = 0usize;
    for segment in raw.split('.') {
        segments += 1;
        let mut chars = segment.chars();
        match chars.next() {
            Some(c) if c.is_ascii_lowercase() => {}
            _ => return false,
        }
        if !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
            return false;
        }
    }
    segments >= 2
}

fn parse_capability(raw: &str) -> Result<ParsedScope, InvalidScope> {
    if !has_capability_shape(raw) {
        return Err(InvalidScope::new(raw, "invalid capability name shape"));
    }
    match DocCapability::parse(raw) {
        Some(name) => Ok(ParsedScope::Capability(name)),
        None => Err(InvalidScope::new(raw, format!("unknown capability: {raw}"))),
    }
}
