//! `names.ts`: a file's name as a person may see it and save it.

use epdf_kernel::mirror::is_js_white_space;

/// `HIDDEN`: control characters, and the bidi controls a name can use to
/// look like another. With a right-to-left override (U+202E) where `[RLO]`
/// stands, `invoice[RLO]fdp.exe` shows as `invoiceexe.pdf`.
fn hidden(c: char) -> bool {
    matches!(
        c,
        '\u{0000}'..='\u{001f}'
            | '\u{007f}'..='\u{009f}'
            | '\u{061c}'
            | '\u{200e}'
            | '\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2066}'..='\u{2069}'
    )
}

/// `displayNameOf(file)`: `name` without control characters and bidi
/// controls, trimmed; `attachment` when nothing is left.
pub fn display_name_of(name: &str) -> String {
    let shown: String = name.chars().filter(|c| !hidden(*c)).collect();
    // `String.prototype.trim`: `WhiteSpace` and `LineTerminator`.
    let shown = shown.trim_matches(is_js_white_space);
    if shown.is_empty() { "attachment".to_string() } else { shown.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_characters_go_and_white_space_is_trimmed() {
        assert_eq!(display_name_of("invoice\u{202e}fdp.exe"), "invoicefdp.exe");
        assert_eq!(display_name_of(" \u{200e}page two\u{202a}.txt "), "page two.txt");
        assert_eq!(display_name_of("a\u{0}b\u{9f}c\u{61c}d\u{2069}e"), "abcde");
        // U+00A0 and U+3000 are white space to `trim`, and stay inside a name.
        assert_eq!(display_name_of("\u{a0}\u{feff}a\u{3000}b\u{3000}"), "a\u{3000}b");
    }

    #[test]
    fn nothing_left_is_attachment() {
        assert_eq!(display_name_of(""), "attachment");
        assert_eq!(display_name_of(" \u{7}\u{200f}\u{2066} "), "attachment");
    }
}
