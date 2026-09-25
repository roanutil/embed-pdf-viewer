//! `shortcuts.ts`: shortcut parsing, matching and display. The platform is an
//! input (`is_mac`), never read from the environment.
//!
//! Case folding is Unicode's (`to_lowercase`, `to_uppercase`), which is what
//! `toLowerCase` and `toUpperCase` do outside a locale, final sigma included.
//! Lengths are UTF-16 code units, as `String.prototype.length` counts.

use epdf_kernel::Error;
use serde::{Deserialize, Serialize};

use crate::js;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedShortcut {
    /// Lowercase.
    pub key: String,
    /// `Mod`: resolved per platform at match time.
    #[serde(rename = "mod")]
    pub mod_: bool,
    pub ctrl: bool,
    pub meta: bool,
    pub alt: bool,
    pub shift: bool,
}

/// The subset of a `KeyboardEvent` the matcher reads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyStroke {
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    pub ctrl_key: bool,
    pub meta_key: bool,
    pub alt_key: bool,
    pub shift_key: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutOptions {
    pub is_mac: bool,
}

const MODIFIERS: [&str; 9] =
    ["mod", "ctrl", "control", "meta", "cmd", "command", "alt", "option", "shift"];

/// Modifiers and one key joined by `+`. An empty last token means the key is
/// `+` itself (`Ctrl++`, `Ctrl+`, and the empty string all have key `+`).
pub fn parse_shortcut(shortcut: &str) -> Result<ParsedShortcut, Error> {
    let tokens: Vec<&str> = shortcut.split('+').map(js::trim).collect();
    let (last, modifiers) = tokens.split_last().expect("split yields at least one token");
    let key_token = if last.is_empty() { "+" } else { last };
    let mut parsed = ParsedShortcut {
        key: key_token.to_lowercase(),
        mod_: false,
        ctrl: false,
        meta: false,
        alt: false,
        shift: false,
    };
    for token in modifiers {
        let t = token.to_lowercase();
        if t.is_empty() {
            continue;
        }
        if !MODIFIERS.contains(&t.as_str()) {
            return Err(Error::new(format!(
                "[ui-core] unknown modifier \"{token}\" in \"{shortcut}\""
            )));
        }
        match t.as_str() {
            "mod" => parsed.mod_ = true,
            "ctrl" | "control" => parsed.ctrl = true,
            "meta" | "cmd" | "command" => parsed.meta = true,
            "alt" | "option" => parsed.alt = true,
            _ => parsed.shift = true,
        }
    }
    // Unreachable: the key token is never empty, and lowercasing a non-empty
    // string never empties it. Kept because the TypeScript keeps it.
    if parsed.key.is_empty() {
        return Err(Error::new(format!("[ui-core] empty key in shortcut \"{shortcut}\"")));
    }
    Ok(parsed)
}

pub fn match_shortcut(parsed: &ParsedShortcut, stroke: &KeyStroke, opts: ShortcutOptions) -> bool {
    let want_ctrl = parsed.ctrl || (parsed.mod_ && !opts.is_mac);
    let want_meta = parsed.meta || (parsed.mod_ && opts.is_mac);
    if stroke.ctrl_key != want_ctrl || stroke.meta_key != want_meta || stroke.alt_key != parsed.alt
    {
        return false;
    }
    // Only declared shift is enforced for printable keys; an undeclared
    // shift rejects a long key name.
    let long = js::utf16_len(&parsed.key) > 1;
    if parsed.shift && !stroke.shift_key {
        return false;
    }
    if !parsed.shift && stroke.shift_key && long {
        return false;
    }
    if stroke.key.to_lowercase() == parsed.key {
        return true;
    }
    long && stroke.code.as_ref().is_some_and(|code| code.to_lowercase() == parsed.key)
}

/// Display form for menu rows: `⌘K` on mac, `Ctrl+K` elsewhere.
pub fn format_shortcut(shortcut: &str, opts: ShortcutOptions) -> Result<String, Error> {
    let parsed = parse_shortcut(shortcut)?;
    let key = if js::utf16_len(&parsed.key) == 1 {
        parsed.key.to_uppercase()
    } else {
        js::capitalize(&parsed.key)
    };
    if opts.is_mac {
        let mut out = String::new();
        if parsed.ctrl {
            out.push('⌃');
        }
        if parsed.alt {
            out.push('⌥');
        }
        if parsed.shift {
            out.push('⇧');
        }
        if parsed.meta || parsed.mod_ {
            out.push('⌘');
        }
        out.push_str(&key);
        return Ok(out);
    }
    let mut parts: Vec<&str> = Vec::new();
    if parsed.ctrl || parsed.mod_ {
        parts.push("Ctrl");
    }
    if parsed.meta {
        parts.push("Meta");
    }
    if parsed.alt {
        parts.push("Alt");
    }
    if parsed.shift {
        parts.push("Shift");
    }
    parts.push(&key);
    Ok(parts.join("+"))
}
