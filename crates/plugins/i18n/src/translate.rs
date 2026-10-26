//! `translate.ts`: the lookup, a key in and a string out. Pure, with no
//! platform access (`Intl.PluralRules` is [`crate::plural`]).

use std::rc::Rc;

use crate::contract::{
    Locale, Param, Params, TranslateOptions, TranslationDictionary, TranslationNode,
};
use crate::js::{Inherited, JsRecord, number_to_string};
use crate::plural::plural_category;
use crate::strings::embedpdf_strings_for;

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

/// `resolveNode`: a string is the answer; a branch needs a plural form,
/// picked by `params.count`.
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

/// `interpolate(text, params)`: fill in `{name}` slots (`/\{(\w+)\}/g`); a
/// slot without a value stays as it is, so it shows while testing.
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

/// `TranslationSources`: what a lookup reads, the two languages it tries
/// and the app's strings.
#[derive(Clone, Copy)]
pub struct TranslationSources<'a> {
    pub locale: &'a str,
    pub fallback_locale: &'a str,
    /// The app's loaded languages, by code.
    pub locales: &'a JsRecord<Rc<Locale>>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct TranslateResult {
    pub text: String,
    /// False when no language has the key: `text` is then the fallback or
    /// the key.
    pub found: bool,
}

/// `stringIn`: a key in one language, the app's string, then EmbedPDF's own.
fn string_in<'a>(
    sources: &TranslationSources<'a>,
    code: &str,
    key: &str,
    params: Option<&Params>,
) -> Option<&'a str> {
    let app = sources.locales.get(code).map(|locale| &locale.translations);
    resolve_node(lookup(app, key), code, params)
        .or_else(|| resolve_node(lookup(embedpdf_strings_for(code), key), code, params))
}

/// `translate(sources, key, options)`: a key's string in the current
/// language (the key, then `options.fallback_key`), then the same in the
/// fallback language, then `options.fallback` (with its slots filled too),
/// then the key itself.
pub fn translate(
    sources: &TranslationSources<'_>,
    key: &str,
    options: Option<&TranslateOptions>,
) -> TranslateResult {
    let params = options.and_then(|o| o.params.as_ref());
    let fallback_key = options.and_then(|o| o.fallback_key.as_deref());
    let in_language = |code: &str| {
        string_in(sources, code, key, params)
            .or_else(|| fallback_key.and_then(|k| string_in(sources, code, k, params)))
    };
    let text = in_language(sources.locale).or_else(|| in_language(sources.fallback_locale));
    if let Some(text) = text {
        return TranslateResult { text: interpolate(text, params), found: true };
    }
    if let Some(fallback) = options.and_then(|o| o.fallback.as_ref()) {
        return TranslateResult { text: interpolate(fallback, params), found: false };
    }
    TranslateResult { text: key.to_string(), found: false }
}
