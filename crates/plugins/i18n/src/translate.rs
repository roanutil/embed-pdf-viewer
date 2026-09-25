//! `translate.ts`: the pure lookup core. State in, string out.

use crate::js::{Inherited, number_to_string};
use crate::plural::plural_category;
use crate::types::{
    I18nState, Param, Params, TranslateOptions, TranslationDictionary, TranslationNode,
};

/// What a dotted-path walk can land on. Beyond the dictionary's own nodes, a
/// JavaScript walk also reaches what every object inherits: `node['toString']`
/// is a function, and a function's `name` is a string. Those are modelled as
/// far as a string can come out of them (`<method>.name`, `__proto__.<method>`);
/// anything further (`toString.call.name`, `constructor.keys.name`) reads as
/// absent here.
#[derive(Clone, Copy, Debug)]
enum Node<'a> {
    Text(&'a str),
    Branch(&'a TranslationDictionary),
    Inherited(Inherited),
}

fn member<'a>(dict: &'a TranslationDictionary, part: &str) -> Option<Node<'a>> {
    match dict.get(part) {
        Some(TranslationNode::Text(text)) => Some(Node::Text(text)),
        Some(TranslationNode::Branch(branch)) => Some(Node::Branch(branch)),
        None => Inherited::of(part).map(Node::Inherited),
    }
}

/// Walk a dotted path ('commands.zoom.in') through a dictionary tree.
fn lookup<'a>(dict: Option<&'a TranslationDictionary>, key: &str) -> Option<Node<'a>> {
    let mut node = Node::Branch(dict?);
    for part in key.split('.') {
        node = match node {
            Node::Text(_) => return None,
            Node::Branch(dict) => member(dict, part)?,
            Node::Inherited(Inherited::ObjectPrototype) => {
                Node::Inherited(Inherited::of(part).filter(|i| *i != Inherited::ObjectPrototype)?)
            }
            Node::Inherited(Inherited::Function { name }) if part == "name" => Node::Text(name),
            Node::Inherited(Inherited::Function { .. }) => return None,
        };
    }
    Some(node)
}

/// A leaf hit is a string; a branch needs a plural pick via `count`.
fn resolve_node<'a>(
    node: Option<Node<'a>>,
    locale: &str,
    params: Option<&Params>,
) -> Option<&'a str> {
    match node? {
        Node::Text(text) => Some(text),
        Node::Branch(branch) => {
            let Some(Param::Number(count)) = params.and_then(|p| p.get("count")) else {
                return None;
            };
            let picked =
                branch.get(plural_category(locale, *count)).or_else(|| branch.get("other"));
            match picked {
                Some(TranslationNode::Text(text)) => Some(text),
                _ => None,
            }
        }
        // A function or `Object.prototype` has no plural members.
        Node::Inherited(_) => None,
    }
}

fn param_to_string(param: &Param) -> String {
    match param {
        Param::Text(text) => text.clone(),
        Param::Number(n) => number_to_string(*n),
    }
}

/// Replace `{slot}` markers (`/\{(\w+)\}/g`); unknown slots stay verbatim.
pub fn interpolate(text: &str, params: Option<&Params>) -> String {
    let Some(params) = params else { return text.to_string() };
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let slot_len =
            after.bytes().take_while(|b| b.is_ascii_alphanumeric() || *b == b'_').count();
        if slot_len > 0 && after.as_bytes().get(slot_len) == Some(&b'}') {
            let slot = &after[..slot_len];
            match params.get(slot) {
                Some(param) => out.push_str(&param_to_string(param)),
                None => match Inherited::of(slot) {
                    Some(inherited) => out.push_str(&inherited.to_js_string()),
                    None => {
                        out.push('{');
                        out.push_str(slot);
                        out.push('}');
                    }
                },
            }
            rest = &after[slot_len + 1..];
        } else {
            out.push('{');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct TranslateResult {
    pub text: String,
    /// False when the key missed every pack (text is then the fallback or the key).
    pub found: bool,
}

/// Resolve a key: current locale → fallback locale → `options.fallback`
/// (interpolated too) → the key itself.
pub fn translate(
    state: &I18nState,
    key: &str,
    options: Option<&TranslateOptions>,
) -> TranslateResult {
    let params = options.and_then(|o| o.params.as_ref());
    let pack = |code: &str| state.locales.get(code).map(|l| &l.translations);
    let text =
        resolve_node(lookup(pack(&state.locale), key), &state.locale, params).or_else(|| {
            resolve_node(lookup(pack(&state.fallback_locale), key), &state.fallback_locale, params)
        });
    if let Some(text) = text {
        return TranslateResult { text: interpolate(text, params), found: true };
    }
    if let Some(fallback) = options.and_then(|o| o.fallback.as_ref()) {
        return TranslateResult { text: interpolate(fallback, params), found: false };
    }
    TranslateResult { text: key.to_string(), found: false }
}
