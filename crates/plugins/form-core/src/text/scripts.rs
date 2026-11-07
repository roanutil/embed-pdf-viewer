//! `scripts.ts`: a character's script (the Unicode `Script` property, as
//! HarfBuzz reads it), and which scripts the engine shapes with Latin's
//! features off.
//!
//! The TypeScript asks the runtime's regular expressions (`\p{Script=…}`) for
//! each name in its list and calls a character of any other script
//! `Unknown`; this reads `unicode-script`'s tables and does the same with
//! the same list, so a script either side doesn't know counts as `Unknown`.

use unicode_script::{Script, UnicodeScript};

const COMMON: &str = "Common";
const INHERITED: &str = "Inherited";
const UNKNOWN: &str = "Unknown";

/// Scripts whose shaping is a matter of default features only: the engine
/// turns kerning and ligatures off for them, as Acrobat does.
const SIMPLE: [&str; 13] = [
    "Latin", "Greek", "Cyrillic", "Armenian", "Georgian", "Han", "Hiragana", "Katakana", "Hangul",
    "Bopomofo", COMMON, INHERITED, UNKNOWN,
];

/// `SCRIPTS`: every script the TypeScript names.
const SCRIPTS: &str = "Common Inherited Latin Hebrew Arabic Cyrillic Greek Han Hiragana Katakana \
    Hangul Devanagari Thai Adlam Ahom Anatolian_Hieroglyphs Armenian Avestan Balinese Bamum \
    Bassa_Vah Batak Bengali Bhaiksuki Bopomofo Brahmi Braille Buginese Buhid Canadian_Aboriginal \
    Carian Caucasian_Albanian Chakma Cham Cherokee Chorasmian Coptic Cuneiform Cypriot \
    Cypro_Minoan Deseret Dives_Akuru Dogra Duployan Egyptian_Hieroglyphs Elbasan Elymaic Ethiopic \
    Garay Georgian Glagolitic Gothic Grantha Gujarati Gunjala_Gondi Gurmukhi Gurung_Khema \
    Hanifi_Rohingya Hanunoo Hatran Imperial_Aramaic Inscriptional_Pahlavi Inscriptional_Parthian \
    Javanese Kaithi Kannada Kawi Kayah_Li Kharoshthi Khitan_Small_Script Khmer Khojki Khudawadi \
    Kirat_Rai Lao Lepcha Limbu Linear_A Linear_B Lisu Lycian Lydian Mahajani Makasar Malayalam \
    Mandaic Manichaean Marchen Masaram_Gondi Medefaidrin Meetei_Mayek Mende_Kikakui \
    Meroitic_Cursive Meroitic_Hieroglyphs Miao Modi Mongolian Mro Multani Myanmar Nabataean \
    Nag_Mundari Nandinagari New_Tai_Lue Newa Nko Nushu Nyiakeng_Puachue_Hmong Ogham Ol_Chiki \
    Ol_Onal Old_Hungarian Old_Italic Old_North_Arabian Old_Permic Old_Persian Old_Sogdian \
    Old_South_Arabian Old_Turkic Old_Uyghur Oriya Osage Osmanya Pahawh_Hmong Palmyrene Pau_Cin_Hau \
    Phags_Pa Phoenician Psalter_Pahlavi Rejang Runic Samaritan Saurashtra Sharada Shavian Siddham \
    SignWriting Sinhala Sogdian Sora_Sompeng Soyombo Sundanese Sunuwar Syloti_Nagri Syriac Tagalog \
    Tagbanwa Tai_Le Tai_Tham Tai_Viet Takri Tamil Tangsa Tangut Telugu Thaana Tibetan Tifinagh \
    Tirhuta Todhri Toto Tulu_Tigalari Ugaritic Vai Vithkuqi Wancho Warang_Citi Yezidi Yi \
    Zanabazar_Square";

/// The script of `code_point`: `"Common"`, `"Inherited"`, `"Unknown"` or a
/// script's name. A lone surrogate is `"Unknown"`.
pub fn script_of(code_point: u32) -> &'static str {
    let Some(character) = char::from_u32(code_point) else { return UNKNOWN };
    let script = character.script();
    if script == Script::Unknown {
        return UNKNOWN;
    }
    let name = script.full_name();
    if SCRIPTS.split_whitespace().any(|known| known == name) { name } else { UNKNOWN }
}

/// Spaces, digits, punctuation, marks: characters that take their
/// neighbours' script.
pub(crate) fn is_common_script(script: &str) -> bool {
    script == COMMON || script == INHERITED || script == UNKNOWN
}

pub fn is_simple_script(script: &str) -> bool {
    SIMPLE.contains(&script)
}
