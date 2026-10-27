//! What `packages/core/fonts/test/standard-fonts.test.ts` checks, over the
//! Rust API: the layout numbers are the engine's, and each program is an
//! OpenType font whose characters reach the glyphs, and advances, the engine
//! lays out with. The vectors pin every value; these pin the relations.

use std::collections::BTreeMap;

use epdf_fonts::{
    FontStyle, StandardFamily, StandardFont, font_program_family, standard_font_css,
    standard_font_metrics, standard_font_program,
};

/// Just enough of an OpenType reader: the tables, a cmap lookup, the advances.
struct OpenType<'a> {
    bytes: &'a [u8],
    tables: Vec<(String, u32, usize, usize)>,
}

impl<'a> OpenType<'a> {
    fn read(bytes: &'a [u8]) -> Self {
        let mut font = OpenType { bytes, tables: Vec::new() };
        for index in 0..font.u16(4) {
            let at = 12 + usize::from(index) * 16;
            let tag = String::from_utf8(bytes[at..at + 4].to_vec()).expect("tag");
            let checksum = font.u32(at + 4);
            font.tables.push((
                tag,
                checksum,
                font.u32(at + 8) as usize,
                font.u32(at + 12) as usize,
            ));
        }
        font
    }

    fn u16(&self, at: usize) -> u16 {
        u16::from_be_bytes([self.bytes[at], self.bytes[at + 1]])
    }

    fn i16(&self, at: usize) -> i16 {
        self.u16(at) as i16
    }

    fn u32(&self, at: usize) -> u32 {
        u32::from_be_bytes(self.bytes[at..at + 4].try_into().expect("4 bytes"))
    }

    fn sum(&self, offset: usize, length: usize) -> u32 {
        (offset..offset + length).step_by(4).fold(0u32, |total, at| {
            let word = (0..4)
                .fold(0u32, |w, i| (w << 8) | u32::from(*self.bytes.get(at + i).unwrap_or(&0)));
            total.wrapping_add(word)
        })
    }

    fn table(&self, name: &str) -> usize {
        self.tables
            .iter()
            .find(|(tag, ..)| tag == name)
            .unwrap_or_else(|| panic!("no {name} table"))
            .2
    }

    fn glyph_of(&self, code_point: u32) -> u16 {
        let cmap = self.table("cmap");
        let sub = cmap + self.u32(cmap + 8) as usize;
        let seg_count = usize::from(self.u16(sub + 6) / 2);
        let ends = sub + 14;
        let starts = ends + seg_count * 2 + 2;
        let deltas = starts + seg_count * 2;
        for index in 0..seg_count {
            let start = u32::from(self.u16(starts + index * 2));
            let end = u32::from(self.u16(ends + index * 2));
            if (start..=end).contains(&code_point) {
                return (code_point as i32 + i32::from(self.i16(deltas + index * 2))) as u16;
            }
        }
        0
    }

    fn advance_of(&self, glyph: u16) -> u32 {
        u32::from(self.u16(self.table("hmtx") + usize::from(glyph) * 4))
    }
}

#[test]
fn lay_text_out_with_the_built_in_faces_numbers() {
    let helvetica = standard_font_metrics(StandardFont::Helvetica);
    // The engine puts a 10 pt line's baseline in an 18 pt plate at
    // 9 − (9.45 − 2.24) / 2 + 1 = 6.395: that is this ascent and descent.
    assert_eq!((helvetica.ascent, helvetica.descent), (945, -224));
    let advances: Vec<_> = "Aa W".chars().map(|c| helvetica.advance(c as u32)).collect();
    assert_eq!(advances, [Some(667), Some(556), Some(278), Some(944)]);
    // WinAnsi can't write it: the engine draws it with another font.
    assert_eq!(helvetica.advance('Ж' as u32), None);
    // A fixed-pitch face advances 600 for every character it writes.
    let courier = standard_font_metrics(StandardFont::CourierBold);
    for c in ['i', 'W', '€', ' '] {
        assert_eq!(courier.advance(c as u32), Some(600), "{c}");
    }
}

#[test]
fn every_face_is_an_open_type_font_with_the_engine_advances() {
    for font in StandardFont::ALL {
        let program = standard_font_program(font);
        let bytes = program.load();
        assert!(std::ptr::eq(bytes, program.load()), "{font}: the same bytes every time");

        let otf = OpenType::read(bytes);
        assert_eq!(&bytes[..4], b"OTTO", "{font}");
        let names: Vec<&str> = otf.tables.iter().map(|(tag, ..)| tag.as_str()).collect();
        assert_eq!(names, ["CFF ", "OS/2", "cmap", "head", "hhea", "hmtx", "maxp", "name", "post"]);
        for (name, checksum, offset, length) in &otf.tables {
            if name != "head" {
                assert_eq!(otf.sum(*offset, *length), *checksum, "{font} {name}");
            }
        }
        assert_eq!(otf.sum(0, bytes.len()), 0xb1b0afba, "{font}");

        let metrics = standard_font_metrics(font);
        let hhea = otf.table("hhea");
        assert_eq!(
            (i32::from(otf.i16(hhea + 4)), i32::from(otf.i16(hhea + 6))),
            (metrics.ascent, metrics.descent),
            "{font}"
        );
        // Every character the engine writes reaches a glyph with the engine's advance.
        let sample = match font {
            StandardFont::Symbol => "αβ∑",
            StandardFont::ZapfDingbats => "✁✂",
            _ => "Hég€",
        };
        for c in sample.chars() {
            let glyph = otf.glyph_of(c as u32);
            assert!(glyph > 0, "{font} {c}");
            if font != StandardFont::ZapfDingbats {
                assert_eq!(Some(otf.advance_of(glyph)), metrics.advance(c as u32), "{font} {c}");
            }
        }
    }
}

#[test]
fn name_each_program_by_its_hash_in_css_with_a_stack_for_before_it_loads() {
    let css = standard_font_css(StandardFont::TimesBoldItalic);
    let hash = standard_font_program(StandardFont::TimesBoldItalic).hash;
    assert_eq!(
        css.font_family,
        format!("\"epdf-{}\", \"Times New Roman\", Times, serif", &hash[..16])
    );
    assert_eq!((css.font_weight, css.font_style), (700, FontStyle::Italic));
}

#[test]
fn programs_and_metrics_are_built_once() {
    for font in StandardFont::ALL {
        assert!(std::ptr::eq(standard_font_program(font), standard_font_program(font)));
        assert!(std::ptr::eq(standard_font_metrics(font), standard_font_metrics(font)));
    }
}

#[test]
fn every_face_has_its_own_program() {
    let mut families = BTreeMap::new();
    for font in StandardFont::ALL {
        let program = standard_font_program(font);
        assert_eq!(program.family, font_program_family(program.hash));
        assert_eq!(families.insert(program.family.clone(), font), None, "{font}");
    }
}

#[test]
fn names_parse_exactly() {
    for font in StandardFont::ALL {
        assert_eq!(font.as_str().parse::<StandardFont>(), Ok(font));
        assert_eq!(serde_json::to_value(font).unwrap(), font.as_str());
    }
    for name in ["Helvetica", "times", "helvetica ", ""] {
        assert!(name.parse::<StandardFont>().is_err(), "{name:?}");
    }
    assert_eq!(StandardFont::ZapfDingbats.face().family, StandardFamily::ZapfDingbats);
}

#[test]
fn a_program_family_slices_utf16_units() {
    assert_eq!(font_program_family("abc"), "epdf-abc");
    assert_eq!(font_program_family(&"é".repeat(20)), format!("epdf-{}", "é".repeat(16)));
    // An astral character counts two units and isn't split.
    assert_eq!(
        font_program_family(&format!("{}😀", "a".repeat(15))),
        format!("epdf-{}", "a".repeat(15))
    );
}
