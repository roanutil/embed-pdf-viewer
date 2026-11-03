//! The scope grammar.
//!
//! A transcription of `packages/engine/core/src/auth/scope/parser.ts`.

use super::types::{
    CollabAction, CollabActionPattern, CollabEntity, CollabFilter, DocCapability, FieldAction,
    FieldActionPattern, FieldFilter, ParsedScope,
};

/// A scope string that matches no known shape.
///
/// Carries the offending string verbatim, as `InvalidScope` does, so a caller
/// can echo it. The vectors compare the [`Display`](std::fmt::Display) text,
/// `reason` included, against TypeScript's message.
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
/// colon is a collab scope even when malformed, a dotted word outside the
/// core namespaces is a plugin permission, and only what is left is tried as
/// a capability. A parser that tried capability first would report "invalid
/// capability name shape" for `annotations:create`.
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
    if let Some(plugin) = plugin_of_permission(raw) {
        return parse_plugin_permission(raw, plugin);
    }
    parse_capability(raw)
}

/// The first segments the core owns (`CORE_NAMESPACES`), across every
/// vocabulary a plugin shares with it: the permission namespaces, then the
/// event namespaces. A plugin id can never be one of these, so a dotted word
/// that starts with one is the core's and must be a word the core knows.
pub const CORE_NAMESPACES: [&str; 14] = [
    "doc",
    "docs",
    "pdf",
    "annotations",
    "fields",
    "tokens",
    "grants",
    "shares",
    "tenants",
    "layer",
    "visitor",
    "document",
    "submission",
    "signature",
];

/// The plugin a permission belongs to (`pluginOfPermission`): its first
/// segment, when the word is dotted and that segment is not one of the
/// core's namespaces. `None` for every core word, and for anything that is
/// not dotted or carries a colon.
pub fn plugin_of_permission(raw: &str) -> Option<&str> {
    if raw.contains(':') {
        return None;
    }
    // `indexOf('.')`; a dot at 0 is no namespace. The byte index is a char
    // boundary because '.' is ASCII.
    let dot = raw.find('.')?;
    if dot == 0 {
        return None;
    }
    let namespace = &raw[..dot];
    (!CORE_NAMESPACES.contains(&namespace)).then_some(namespace)
}

/// Whether `id` can be a plugin's id (`isPluginId`): lowercase words joined
/// by dashes, and not one of [`CORE_NAMESPACES`].
pub fn is_plugin_id(id: &str) -> bool {
    has_plugin_word_shape(id) && !CORE_NAMESPACES.contains(&id)
}

/// `^[a-z][a-z0-9]*(-[a-z0-9]+)*$` (`PLUGIN_ID_SHAPE`), which each segment of
/// a plugin permission also has to match.
fn has_plugin_word_shape(word: &str) -> bool {
    let mut parts = word.split('-');
    let first = parts.next().unwrap_or_default();
    let mut chars = first.chars();
    if !chars.next().is_some_and(|c| c.is_ascii_lowercase()) {
        return false;
    }
    let tail = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit();
    chars.all(tail) && parts.all(|part| !part.is_empty() && part.chars().all(tail))
}

/// `^[a-z][a-z0-9]*(-[a-z0-9]+)*(\.[a-z][a-z0-9]*(-[a-z0-9]+)*)+$`
/// (`PLUGIN_PERMISSION_SHAPE`): a plugin id, then one or more dotted actions
/// of the same form.
fn has_plugin_permission_shape(raw: &str) -> bool {
    let mut segments = 0usize;
    for segment in raw.split('.') {
        segments += 1;
        if !has_plugin_word_shape(segment) {
            return false;
        }
    }
    segments >= 2
}

fn parse_plugin_permission(raw: &str, plugin: &str) -> Result<ParsedScope, InvalidScope> {
    if !has_plugin_permission_shape(raw) {
        return Err(InvalidScope::new(
            raw,
            "invalid plugin permission shape (expected <plugin id>.<action>)",
        ));
    }
    Ok(ParsedScope::Plugin { name: raw.to_string(), plugin: plugin.to_string() })
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

    if entity == "fields" {
        return parse_field_scope(raw, action, filter_str);
    }
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

/// `fields:<fill|sign|set-group|*>:<all|group=NAME>` (`parseFieldScope`).
/// A field's group is who fills it, and a field records no creator, so
/// `self` and `createdBy=` are refused.
fn parse_field_scope(
    raw: &str,
    action: &str,
    filter_str: &str,
) -> Result<ParsedScope, InvalidScope> {
    let action = match action {
        "fill" => FieldActionPattern::One(FieldAction::Fill),
        "sign" => FieldActionPattern::One(FieldAction::Sign),
        "set-group" => FieldActionPattern::One(FieldAction::SetGroup),
        "*" => FieldActionPattern::Any,
        other => return Err(InvalidScope::new(raw, format!("unknown fields action: {other}"))),
    };
    let filter = match parse_filter(filter_str, raw)? {
        CollabFilter::All => FieldFilter::All,
        CollabFilter::Group(group) => FieldFilter::Group(group),
        other => {
            return Err(InvalidScope::new(
                raw,
                format!("fields only support :all or :group=<id> filters (got :{})", other.kind()),
            ));
        }
    };
    Ok(ParsedScope::FieldCollab { action, filter })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_of_permission_names_the_plugin_of_a_plugin_word_only() {
        assert_eq!(plugin_of_permission("protected-display.view"), Some("protected-display"));
        for core in
            ["doc.render", "pdf.permissions", "grants.manage", "annotations:create:self", "*"]
        {
            assert_eq!(plugin_of_permission(core), None, "{core}");
        }
        assert_eq!(plugin_of_permission(".view"), None);
        assert_eq!(plugin_of_permission("snapshot"), None);
    }

    #[test]
    fn every_core_namespace_is_reserved() {
        for namespace in CORE_NAMESPACES {
            assert_eq!(plugin_of_permission(&format!("{namespace}.anything")), None);
            assert!(!is_plugin_id(namespace), "{namespace}");
        }
    }

    #[test]
    fn a_plugin_id_is_lowercase_words_joined_by_dashes() {
        for id in ["protected-display", "snapshot", "ai-chat", "stamp2", "a-1b"] {
            assert!(is_plugin_id(id), "{id}");
        }
        for id in [
            "",
            "Protected",
            "protected_display",
            "protected--display",
            "protected-",
            "-x",
            "1x",
            "a.b",
        ] {
            assert!(!is_plugin_id(id), "{id}");
        }
    }
}
