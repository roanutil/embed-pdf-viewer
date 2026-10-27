//! G1. Injected identifier source.
//!
//! Replaces `services/src/shared/uuid.ts`. Injected rather than ambient for
//! the same reason as [`crate::clock`]: the cross-language corpus cannot
//! freeze a result that contains a fresh random id.
//!
//! The format is RFC 4122 v4, canonical lowercase hex
//! `xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx` where `y` is one of `8`, `9`, `a`,
//! `b`. That is the observable contract the TypeScript version has, and the
//! bit-twiddling that produces it is the whole of what ports; the entropy
//! source does not, because `globalThis.crypto.getRandomValues` is a host
//! object.

/// The engine's only source of fresh identifiers.
pub trait IdSource: std::fmt::Debug {
    /// 16 random bytes. Formatting them as a UUID is [`uuid_v4_from_bytes`],
    /// which is shared by every implementation so the format cannot drift.
    fn fill_random(&self, bytes: &mut [u8; 16]);

    fn next_uuid_v4(&self) -> String {
        let mut bytes = [0u8; 16];
        self.fill_random(&mut bytes);
        uuid_v4_from_bytes(bytes)
    }
}

/// The host's cryptographic random source.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemIdSource;

impl IdSource for SystemIdSource {
    fn fill_random(&self, bytes: &mut [u8; 16]) {
        // A failure here means the platform has no entropy at all, which is
        // not a condition an engine operation can carry on through.
        getrandom::fill(bytes).expect("no system entropy source");
    }
}

/// A deterministic source for corpus runs and tests.
///
/// SplitMix64, chosen because it is eight lines and reproduces byte for byte
/// on every target. Not for anything that needs unpredictability.
#[derive(Debug)]
pub struct SeededIdSource {
    state: std::cell::Cell<u64>,
}

impl SeededIdSource {
    pub fn new(seed: u64) -> Self {
        Self { state: std::cell::Cell::new(seed) }
    }

    fn next_u64(&self) -> u64 {
        let mut z = self.state.get().wrapping_add(0x9E37_79B9_7F4A_7C15);
        self.state.set(z);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

impl IdSource for SeededIdSource {
    fn fill_random(&self, bytes: &mut [u8; 16]) {
        bytes[..8].copy_from_slice(&self.next_u64().to_le_bytes());
        bytes[8..].copy_from_slice(&self.next_u64().to_le_bytes());
    }
}

/// Applies RFC 4122 §4.4's version and variant nibbles and renders the
/// canonical hex form.
pub fn uuid_v4_from_bytes(mut bytes: [u8; 16]) -> String {
    bytes[6] = (bytes[6] & 0x0f) | 0x40; // version 4
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // variant 10xx

    let mut out = String::with_capacity(36);
    for (i, byte) in bytes.iter().enumerate() {
        if matches!(i, 4 | 6 | 8 | 10) {
            out.push('-');
        }
        out.push(char::from_digit((byte >> 4) as u32, 16).unwrap());
        out.push(char::from_digit((byte & 0x0f) as u32, 16).unwrap());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_the_canonical_v4_shape_from_known_bytes() {
        let bytes = [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
            0x0e, 0x0f,
        ];
        assert_eq!(uuid_v4_from_bytes(bytes), "00010203-0405-4607-8809-0a0b0c0d0e0f");
    }

    #[test]
    fn stamps_version_and_variant_over_whatever_the_source_gave() {
        // All-ones input: the only nibbles that may differ from `f` are the
        // version nibble and the top two bits of the variant nibble.
        assert_eq!(uuid_v4_from_bytes([0xff; 16]), "ffffffff-ffff-4fff-bfff-ffffffffffff");
        assert_eq!(uuid_v4_from_bytes([0x00; 16]), "00000000-0000-4000-8000-000000000000");
    }

    #[test]
    fn a_seeded_source_repeats_exactly_for_the_same_seed() {
        let first: Vec<String> = (0..3).map(|_| SeededIdSource::new(7).next_uuid_v4()).collect();
        let source = SeededIdSource::new(7);
        let sequence: Vec<String> = (0..3).map(|_| source.next_uuid_v4()).collect();

        // Each fresh source at seed 7 gives the same first value...
        assert!(first.iter().all(|id| *id == first[0]));
        // ...and one source advances rather than repeating.
        assert_eq!(sequence[0], first[0]);
        assert_ne!(sequence[0], sequence[1]);
        assert_ne!(sequence[1], sequence[2]);
    }

    #[test]
    fn the_system_source_produces_well_formed_distinct_ids() {
        let source = SystemIdSource;
        let a = source.next_uuid_v4();
        let b = source.next_uuid_v4();
        assert_ne!(a, b);
        assert_eq!(a.len(), 36);
        assert_eq!(a.as_bytes()[14], b'4');
        assert!(matches!(a.as_bytes()[19], b'8' | b'9' | b'a' | b'b'));
        assert!(a.chars().all(|c| c == '-' || c.is_ascii_hexdigit()));
    }
}
