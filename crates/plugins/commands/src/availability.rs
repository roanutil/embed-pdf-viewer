//! `services/availability.ts`: whether a command is there at all, apart
//! from what it says about the moment (`enabled`, `visible`): turned off in
//! this viewer (its id, or one of its categories), or not allowed for its
//! document (the scope's `requires`, or the plugins' checks in `allowed`),
//! and the sentence that says why, translated when the i18n plugin is
//! there.

use epdf_kernel::engine::DocumentSecurity;
use epdf_plugin_i18n::{I18N, Param, Params, TranslateOptions, Translator, interpolate};

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

/// `Translate`: how a command's sentences are translated, the i18n
/// plugin's `t`, or `None` without the plugin.
pub type Translate = Option<Translator>;

/// `translateOf`: the translate function for a command's document, a new
/// one only when its strings may differ.
pub fn translate_of(context: &CommandCtx) -> Translate {
    context.try_get(&I18N).map(|i18n| i18n.get_translator())
}

/// `labelOf`: a language's string under the command's id, else under its
/// `labelKey` (the current language, then the fallback one), else `label`,
/// else the id, with `labelParams` in its slots.
pub fn label_of(def: &CommandDef, params: Option<&Params>, t: &Translate) -> String {
    let Some(t) = t else {
        let text = def.label.as_deref().or(def.label_key.as_deref()).unwrap_or(&def.id);
        return interpolate(text, params);
    };
    let fallback_key = truthy(&def.label_key).map(str::to_string);
    let params = params.cloned();
    // A command with a key and no label of its own warns once when no
    // language has the key.
    if def.label.is_none() && fallback_key.is_some() {
        let options = TranslateOptions { fallback_key, params, ..TranslateOptions::default() };
        return t(&def.id, Some(&options));
    }
    let fallback = def.label.clone().unwrap_or_else(|| def.id.clone());
    let options = TranslateOptions { fallback_key, fallback: Some(fallback), params };
    t(&def.id, Some(&options))
}

/// `labelParamsOf`: the values for a label's slots, from `labelParams`;
/// none without it, or when it throws.
pub fn label_params_of(def: &CommandDef, context: &CommandCtx) -> Option<Params> {
    def.label_params.as_ref().and_then(|f| f(context).ok())
}

/// `JSON.stringify(labelParams)`: the params as a memo compares them, so
/// equal values compare equal (a number that isn't finite is `null`).
pub fn params_json(params: &Params) -> String {
    let mut out = String::from("{");
    for (index, (key, value)) in params.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&serde_json::Value::from(key).to_string());
        out.push(':');
        match value {
            Param::Text(text) => out.push_str(&serde_json::Value::from(text.as_str()).to_string()),
            Param::Number(n) if n.is_finite() => {
                out.push_str(&epdf_kernel::js::number_to_string(*n))
            }
            Param::Number(_) => out.push_str("null"),
        }
    }
    out.push('}');
    out
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
pub fn not_allowed_message(key: &str, label: &str, t: &Translate) -> String {
    message(key, DEFAULT_NOT_ALLOWED_KEY, format!("{label} isn't allowed"), label, t)
}

/// `t(key, { params, fallback: t(general, { params, fallback }) })`, or the
/// fallback without the plugin.
fn message(key: &str, general: &str, fallback: String, label: &str, t: &Translate) -> String {
    let Some(t) = t else { return fallback };
    let general = t(
        general,
        Some(&TranslateOptions {
            params: Some(label_params(label)),
            fallback: Some(fallback),
            fallback_key: None,
        }),
    );
    t(
        key,
        Some(&TranslateOptions {
            params: Some(label_params(label)),
            fallback: Some(general),
            fallback_key: None,
        }),
    )
}

/// `turnedOffMessage`: the sentence for a command that's turned off.
fn turned_off_message(label: &str, t: &Translate) -> String {
    match t {
        Some(t) => t(
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
    let t = translate_of(context);
    let label = || label_of(def, label_params_of(def, context).as_ref(), &t);
    if is_turned_off(def, settings) {
        return Some(CommandRefusal {
            reason: RefusalReason::TurnedOff,
            message: turned_off_message(&label(), &t),
        });
    }
    if let Some(key) = not_allowed_key_of(def, context) {
        return Some(CommandRefusal {
            reason: RefusalReason::NotAllowed,
            message: not_allowed_message(&key, &label(), &t),
        });
    }
    locked_key_of(def, context).map(|key| CommandRefusal {
        reason: RefusalReason::Locked,
        message: locked_message(&key, &label(), &t),
    })
}

/// The general locked sentence's key: `{label} can't be changed in this
/// document`.
const DEFAULT_LOCKED_KEY: &str = "commands.locked.default";

/// `lockedKeyOf`: why a lock holds the setting a command changes, as the
/// key of the sentence that says so (the lock's `reasonKey`, else
/// `commands.locked.default`), or `None` while nothing holds it; a `locked`
/// that throws is no lock.
pub fn locked_key_of(def: &CommandDef, context: &CommandCtx) -> Option<String> {
    let lock = def.locked.as_ref().and_then(|locked| locked(context).ok().flatten())?;
    Some(lock.reason_key.unwrap_or_else(|| DEFAULT_LOCKED_KEY.to_string()))
}

/// `lockedMessage`: the sentence for a command whose setting a lock holds,
/// under `key`, else the general one.
pub fn locked_message(key: &str, label: &str, t: &Translate) -> String {
    message(key, DEFAULT_LOCKED_KEY, format!("{label} can't be changed in this document"), label, t)
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
