//! `services/availability.ts`: whether a command is there at all, apart
//! from what it says about the moment (`enabled`, `visible`): turned off in
//! this viewer (its id, or one of its categories), or not allowed for its
//! document (the scope's `requires`, or the plugins' checks in `allowed`),
//! and the sentence that says why, translated when the i18n plugin is
//! there.

use epdf_kernel::engine::DocumentSecurity;
use epdf_plugin_i18n::{I18N, Param, Params, TranslateOptions};

use crate::contract::{CommandCtx, CommandDef, CommandRefusal, CommandsSettings, RefusalReason};

/// `isCategoryOff`: whether a category is off: it, or a category it's
/// under, is in the list. Parts are whole dotted segments, so `annotations`
/// covers `annotations.highlight` and not `annotationsExtra`.
pub fn is_category_off(category: &str, disabled: &[String]) -> bool {
    disabled.iter().any(|off| {
        category == off
            || category.strip_prefix(off.as_str()).is_some_and(|rest| rest.starts_with('.'))
    })
}

/// A JavaScript truthy string member.
fn truthy(value: &Option<String>) -> Option<&str> {
    value.as_deref().filter(|v| !v.is_empty())
}

/// `labelOf`: a language's string under the command's id, else under its
/// `labelKey` (the current language, then the fallback one), else `label`,
/// else the id.
pub fn label_of(def: &CommandDef, context: &CommandCtx) -> String {
    let Some(i18n) = context.try_get(&I18N) else {
        return def
            .label
            .clone()
            .or_else(|| def.label_key.clone())
            .unwrap_or_else(|| def.id.clone());
    };
    let fallback_key = truthy(&def.label_key).map(str::to_string);
    // A command with a key and no label of its own warns once when no
    // language has the key.
    if def.label.is_none() && fallback_key.is_some() {
        let options = TranslateOptions { fallback_key, ..TranslateOptions::default() };
        return i18n.t(&def.id, Some(&options));
    }
    let fallback = def.label.clone().unwrap_or_else(|| def.id.clone());
    let options = TranslateOptions { fallback_key, fallback: Some(fallback), params: None };
    i18n.t(&def.id, Some(&options))
}

/// `sessionHolds`: what `ctx.allows` answers for a document-scoped plugin,
/// asked of the command's document.
fn session_holds(security: &dyn DocumentSecurity, permission: &str) -> bool {
    if permission == "annotations:create" {
        security.allows_annotation_create()
    } else {
        security.allows(permission)
    }
}

/// `permissionKeyOf`: where a language keeps the sentence for a permission,
/// `commands.notAllowed.doc-text-copy`.
fn permission_key_of(permission: &str) -> String {
    format!("commands.notAllowed.{}", permission.replace(['.', ':'], "-"))
}

/// The general sentence's key: `{label} isn't allowed`.
const DEFAULT_NOT_ALLOWED_KEY: &str = "commands.notAllowed.default";

fn label_params(label: &str) -> Params {
    let mut params = Params::new();
    params.insert("label", Param::Text(label.to_string()));
    params
}

/// `notAllowedMessage`: the sentence for a command the session may not
/// run, under `key`, else the general one.
pub fn not_allowed_message(key: &str, label: &str, context: &CommandCtx) -> String {
    let fallback = format!("{label} isn't allowed");
    let Some(i18n) = context.try_get(&I18N) else { return fallback };
    let general = i18n.t(
        DEFAULT_NOT_ALLOWED_KEY,
        Some(&TranslateOptions {
            params: Some(label_params(label)),
            fallback: Some(fallback),
            fallback_key: None,
        }),
    );
    i18n.t(
        key,
        Some(&TranslateOptions {
            params: Some(label_params(label)),
            fallback: Some(general),
            fallback_key: None,
        }),
    )
}

/// `turnedOffMessage`: the sentence for a command that's turned off.
fn turned_off_message(label: &str, context: &CommandCtx) -> String {
    match context.try_get(&I18N) {
        Some(i18n) => i18n.t(
            "commands.turnedOff",
            Some(&TranslateOptions {
                params: Some(label_params(label)),
                fallback: Some("{label} isn't available".into()),
                fallback_key: None,
            }),
        ),
        None => format!("{label} isn't available"),
    }
}

/// `isTurnedOff`: by its id, or through one of its categories.
pub fn is_turned_off(def: &CommandDef, settings: &CommandsSettings) -> bool {
    settings.disabled_commands.contains(&def.id)
        || def
            .categories
            .iter()
            .flatten()
            .any(|category| is_category_off(category, &settings.disabled_categories))
}

/// `passesCheck`: whether a command's `allowed` check says yes; one that
/// throws allows it.
fn passes_check(def: &CommandDef, context: &CommandCtx) -> bool {
    match &def.allowed {
        None => true,
        Some(allowed) => allowed(context).unwrap_or(true),
    }
}

/// `notAllowedKeyOf`: why its document's session may not run a command, as
/// the key of the sentence that says so, or `None` when it may. With no
/// document open there is no session to ask, and nothing is refused.
pub fn not_allowed_key_of(def: &CommandDef, context: &CommandCtx) -> Option<String> {
    let requires = def.requires.as_deref().unwrap_or_default();
    if requires.is_empty() && def.allowed.is_none() {
        return None;
    }
    let handle = context.ctx.document_handle(Some(context.document_id()?))?;
    let security = handle.security();
    if let Some(missing) = requires.iter().find(|p| !session_holds(&*security, p)) {
        return Some(permission_key_of(missing));
    }
    if passes_check(def, context) {
        None
    } else {
        Some(def.not_allowed_key.clone().unwrap_or_else(|| DEFAULT_NOT_ALLOWED_KEY.to_string()))
    }
}

/// `refusalOf`: why a command won't run whatever the moment, or `None`
/// when it may run.
pub fn refusal_of(
    def: &CommandDef,
    context: &CommandCtx,
    settings: &CommandsSettings,
) -> Option<CommandRefusal> {
    if is_turned_off(def, settings) {
        return Some(CommandRefusal {
            reason: RefusalReason::TurnedOff,
            message: turned_off_message(&label_of(def, context), context),
        });
    }
    let key = not_allowed_key_of(def, context)?;
    Some(CommandRefusal {
        reason: RefusalReason::NotAllowed,
        message: not_allowed_message(&key, &label_of(def, context), context),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_match_on_whole_dotted_parts() {
        let off = vec!["annotations".to_string()];
        assert!(is_category_off("annotations", &off));
        assert!(is_category_off("annotations.highlight", &off));
        assert!(is_category_off("annotations.highlight.ink", &off));
        assert!(!is_category_off("annotationsExtra", &off));
        assert!(!is_category_off("annotation", &off));
        assert!(!is_category_off("other.annotations", &off));
    }

    #[test]
    fn a_permission_key_replaces_dots_and_colons() {
        assert_eq!(permission_key_of("doc.text.copy"), "commands.notAllowed.doc-text-copy");
        assert_eq!(
            permission_key_of("annotations:create"),
            "commands.notAllowed.annotations-create"
        );
    }
}
