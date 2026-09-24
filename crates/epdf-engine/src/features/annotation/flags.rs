//! The `/F` annotation flags word.
//!
//! G7. Ports `services/src/features/annotations/internal/annotationFlagBits.ts`,
//! which is ISO 32000 §12.5.3 Table 165.
//!
//! Ten bits, expanded into ten booleans so no caller has to know a position.
//! The table is the single thing the reader and the G11 writer share, so a bit
//! transcribed one place off is wrong in both directions at once and reads
//! back consistently: the document would say `noView` and the wire would say
//! `noRotate`, and nothing downstream could tell. That is why it is pinned by
//! vector rather than by review.

use serde::{Deserialize, Serialize};

/// The `/F` bitset, expanded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationFlags {
    /// Bit 1. Do not render, do not print, even with no handler.
    pub invisible: bool,
    /// Bit 2. Do not render and do not print.
    pub hidden: bool,
    /// Bit 3. Print.
    pub print: bool,
    /// Bit 4. Ignore magnification.
    pub no_zoom: bool,
    /// Bit 5. Ignore page rotation.
    pub no_rotate: bool,
    /// Bit 6. Do not render on screen; may still print.
    pub no_view: bool,
    /// Bit 7. No interaction. Widgets excepted: §12.5.3 defers to `/Ff`.
    pub read_only: bool,
    /// Bit 8. Position and size are fixed.
    pub locked: bool,
    /// Bit 9. Invert `no_view` while the annotation is selected.
    pub toggle_no_view: bool,
    /// Bit 10. Contents are fixed; position and size are not.
    pub locked_contents: bool,
}

/// All ten bits clear, which is what an absent `/F` means.
pub const NO_ANNOTATION_FLAGS: AnnotationFlags = AnnotationFlags {
    invisible: false,
    hidden: false,
    print: false,
    no_zoom: false,
    no_rotate: false,
    no_view: false,
    read_only: false,
    locked: false,
    toggle_no_view: false,
    locked_contents: false,
};

/// Bit positions, in the order `ANNOT_FLAG_BITS` declares them.
///
/// Held as a table rather than as a `match` because the reader, the writer and
/// the vector runner all walk it, and a table cannot have one arm that a
/// refactor forgets.
pub const ANNOT_FLAG_BITS: [(&str, u32); 10] = [
    ("invisible", 1 << 0),
    ("hidden", 1 << 1),
    ("print", 1 << 2),
    ("noZoom", 1 << 3),
    ("noRotate", 1 << 4),
    ("noView", 1 << 5),
    ("readOnly", 1 << 6),
    ("locked", 1 << 7),
    ("toggleNoView", 1 << 8),
    ("lockedContents", 1 << 9),
];

impl AnnotationFlags {
    /// Expand a raw `/F` word.
    ///
    /// Bits above position 10 are not an error and are not reported. §12.5.3
    /// reserves them, PDFium hands back whatever the file holds, and a
    /// document that sets one is asking for a behaviour this engine has no
    /// vocabulary for. Dropping them is the same choice the TypeScript makes.
    pub fn from_bits(bits: u32) -> Self {
        Self {
            invisible: bits & (1 << 0) != 0,
            hidden: bits & (1 << 1) != 0,
            print: bits & (1 << 2) != 0,
            no_zoom: bits & (1 << 3) != 0,
            no_rotate: bits & (1 << 4) != 0,
            no_view: bits & (1 << 5) != 0,
            read_only: bits & (1 << 6) != 0,
            locked: bits & (1 << 7) != 0,
            toggle_no_view: bits & (1 << 8) != 0,
            locked_contents: bits & (1 << 9) != 0,
        }
    }

    /// The word these flags describe, with every reserved bit clear.
    ///
    /// Not the inverse of [`AnnotationFlags::from_bits`] for a word that set a
    /// reserved bit, by construction: `from_bits` dropped it. The G11 writer
    /// therefore folds a patch onto the live word rather than rebuilding it
    /// from a decoded value, and this is the create-from-nothing case.
    pub fn to_bits(self) -> u32 {
        let mut bits = 0;
        for (name, bit) in ANNOT_FLAG_BITS {
            if self.get(name).expect("the bit table names this struct's own fields") {
                bits |= bit;
            }
        }
        bits
    }

    /// One flag by its wire name, for the table-driven callers.
    pub fn get(self, name: &str) -> Option<bool> {
        Some(match name {
            "invisible" => self.invisible,
            "hidden" => self.hidden,
            "print" => self.print,
            "noZoom" => self.no_zoom,
            "noRotate" => self.no_rotate,
            "noView" => self.no_view,
            "readOnly" => self.read_only,
            "locked" => self.locked,
            "toggleNoView" => self.toggle_no_view,
            "lockedContents" => self.locked_contents,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_bit_sets_exactly_its_own_flag() {
        for (name, bit) in ANNOT_FLAG_BITS {
            let flags = AnnotationFlags::from_bits(bit);
            for (other, _) in ANNOT_FLAG_BITS {
                assert_eq!(
                    flags.get(other).unwrap(),
                    other == name,
                    "bit {bit:#x} ({name}) also set {other}",
                );
            }
        }
    }

    #[test]
    fn an_absent_f_entry_is_every_flag_clear() {
        assert_eq!(AnnotationFlags::from_bits(0), NO_ANNOTATION_FLAGS);
        assert_eq!(AnnotationFlags::default(), NO_ANNOTATION_FLAGS);
        assert_eq!(NO_ANNOTATION_FLAGS.to_bits(), 0);
    }

    #[test]
    fn reserved_bits_above_the_tenth_are_dropped_rather_than_reported() {
        // 0x400 is bit 11, the first reserved one.
        assert_eq!(AnnotationFlags::from_bits(0xffff_fc00), NO_ANNOTATION_FLAGS);
        // And a word that sets everything reads as all ten and writes back as
        // only ten, which is the asymmetry `to_bits` documents.
        let all = AnnotationFlags::from_bits(u32::MAX);
        assert!(ANNOT_FLAG_BITS.iter().all(|(name, _)| all.get(name).unwrap()));
        assert_eq!(all.to_bits(), 0x3ff);
    }

    #[test]
    fn the_wire_names_are_what_serde_emits() {
        let value = serde_json::to_value(AnnotationFlags::from_bits(0x3ff)).unwrap();
        let object = value.as_object().unwrap();
        assert_eq!(object.len(), ANNOT_FLAG_BITS.len());
        for (name, _) in ANNOT_FLAG_BITS {
            assert_eq!(object.get(name), Some(&serde_json::json!(true)), "{name}");
        }
    }
}
