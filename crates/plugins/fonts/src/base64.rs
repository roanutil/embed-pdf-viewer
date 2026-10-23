//! `base64.ts`: base64 to bytes, as the package decodes its programs.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// A character's value; `0` for one outside the alphabet, padding included,
/// as the TypeScript's zero-filled table answers.
fn value(byte: u8) -> u32 {
    ALPHABET.iter().position(|&c| c == byte).map_or(0, |at| at as u32)
}

/// Decodes padded base64. `text` is ASCII and a whole number of 4-character
/// groups, as every program is (`build.rs` checks); the TypeScript throws a
/// `RangeError` for a length that isn't.
pub(crate) fn decode_base64(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let padding = if text.ends_with("==") {
        2
    } else if text.ends_with('=') {
        1
    } else {
        0
    };
    let length = (bytes.len() / 4 * 3).saturating_sub(padding);
    let mut out = Vec::with_capacity(length);
    for group in bytes.chunks(4) {
        let at = |index: usize| group.get(index).copied().map_or(0, value);
        let chunk = (at(0) << 18) | (at(1) << 12) | (at(2) << 6) | at(3);
        for byte in [chunk >> 16, (chunk >> 8) & 0xff, chunk & 0xff] {
            if out.len() < length {
                out.push(byte as u8);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::decode_base64;

    #[test]
    fn decodes_each_padding() {
        assert_eq!(decode_base64(""), b"");
        assert_eq!(decode_base64("TQ=="), b"M");
        assert_eq!(decode_base64("TWE="), b"Ma");
        assert_eq!(decode_base64("TWFu"), b"Man");
        assert_eq!(
            decode_base64("/+7dzLuqmYh3ZlVEMyIRAA=="),
            [
                0xff, 0xee, 0xdd, 0xcc, 0xbb, 0xaa, 0x99, 0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22,
                0x11, 0x00
            ]
        );
    }
}
