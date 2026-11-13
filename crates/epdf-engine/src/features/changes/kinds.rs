//! The version each kind of change op and result item arrived in
//! (`core/src/wire/changeKinds.ts` and `apiVersion.ts`, a4805a492).
//!
//! Neither side sends a kind to a side older than the kind: a client holds a
//! change back from an older server, and a server sends an older client
//! `full-refresh` in place of a row it can't read. A new kind gets its row
//! in [`CHANGE_KIND_SINCE`] in the change that adds it.

/// The API version this build speaks (`API_VERSION`): the major and minor of
/// the TypeScript packages it follows, which release together.
/// `vectors/changes/kinds.json` records theirs, and the replay checks this
/// against it.
pub const API_VERSION: &str = "3.2";

/// Every kind of change op and result item, in the order the wire schema
/// lists them (`CHANGE_KINDS`): an op's type, or a restore. An import is a
/// route of its own, so it isn't one.
pub const CHANGE_KINDS: &[&str] = &[
    "annotations.create",
    "annotations.update",
    "annotations.delete",
    "annotations.reorder",
    "annotations.copy",
    "forms.setValue",
    "forms.setDisplay",
    "forms.setAppearanceText",
    "forms.reset",
    "forms.create",
    "forms.copy",
    "forms.update",
    "forms.delete",
    "forms.addWidget",
    "forms.removeWidget",
    "forms.deleteWidget",
    "forms.reorderCalculations",
    "forms.reorderWidgets",
    "forms.updateWidget",
    "forms.setSignatureAppearance",
    "metadata.update",
    "metadata.updateCustom",
    "outline.create",
    "outline.update",
    "outline.move",
    "outline.delete",
    "pages.rotate",
    "pages.setScale",
    "pages.reorder",
    "pages.delete",
    "pages.insertBlank",
    "pages.insert",
    "pages.copy",
    "attachments.create",
    "attachments.delete",
    "annotations.restore",
    "forms.restore",
    "forms.restoreWidget",
    "outline.restore",
    "pages.restore",
    "attachments.restore",
];

/// The version each kind arrived in, when it isn't [`FIRST_VERSION`]
/// (`CHANGE_KIND_SINCE`).
pub const CHANGE_KIND_SINCE: &[(&str, &str)] =
    &[("pages.setScale", "3.1"), ("annotations.copy", "3.2"), ("forms.copy", "3.2")];

/// The version every kind without a row arrived in.
const FIRST_VERSION: &str = "3.0";

/// An API version, `<major>.<minor>` (`parseApiVersion`); `None` for
/// anything else, a patch number included.
pub fn parse_api_version(raw: &str) -> Option<(u64, u64)> {
    let number = |part: &str| -> Option<u64> {
        let leading_zero = part.len() > 1 && part.starts_with('0');
        (!part.is_empty() && !leading_zero && part.bytes().all(|b| b.is_ascii_digit()))
            .then(|| part.parse().ok())
            .flatten()
    };
    let (major, minor) = raw.split_once('.')?;
    Some((number(major)?, number(minor)?))
}

fn is_older(version: (u64, u64), than: (u64, u64)) -> bool {
    version < than
}

/// Whether the other side lacks something that arrived in `since`
/// (`lacksWhatArrivedIn`). `version` is what the other side said it speaks;
/// `None` when it didn't say, which reads as this build's own. `since` is
/// `None` for something this build doesn't have itself, which only a newer
/// side can have.
///
/// # Panics
///
/// When `since` is not an API version, as upstream throws.
pub fn lacks_what_arrived_in(version: Option<&str>, since: Option<&str>) -> bool {
    let own = parse_api_version(API_VERSION).expect("API_VERSION is an API version");
    let other = version.and_then(parse_api_version).unwrap_or(own);
    let Some(since) = since else { return !is_older(own, other) };
    let arrived = parse_api_version(since).unwrap_or_else(|| panic!("not an API version: {since}"));
    is_older(other, own) && is_older(other, arrived)
}

/// Whether `kind` is a kind of change op or result item this build has
/// (`isChangeKind`).
pub fn is_change_kind(kind: &str) -> bool {
    CHANGE_KINDS.contains(&kind)
}

/// The version `kind` arrived in (`changeKindSince`).
pub fn change_kind_since(kind: &str) -> &'static str {
    CHANGE_KIND_SINCE.iter().find(|(k, _)| *k == kind).map_or(FIRST_VERSION, |(_, since)| since)
}

/// The kinds among `kinds` the other side doesn't have, each once, in the
/// order given (`changeKindsUnknownTo`).
pub fn change_kinds_unknown_to<'a>(
    version: Option<&str>,
    kinds: impl IntoIterator<Item = &'a str>,
) -> Vec<&'a str> {
    let mut unknown: Vec<&str> = Vec::new();
    for kind in kinds {
        let since = is_change_kind(kind).then(|| change_kind_since(kind));
        if lacks_what_arrived_in(version, since) && !unknown.contains(&kind) {
            unknown.push(kind);
        }
    }
    unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_api_version_is_a_major_and_a_minor() {
        assert!(parse_api_version("3.2.0").is_none());
        assert!(parse_api_version("03.2").is_none());
        assert_eq!(parse_api_version("10.0"), Some((10, 0)));
    }
}
