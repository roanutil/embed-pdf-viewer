//! `helvetica.ts`: Helvetica as the engine sets it, so a label or caption
//! measured here lays out where the engine will draw it.

/// Standard Helvetica advances in 1/1000 em, ASCII 32 to 126.
const ADVANCES: [f64; 95] = [
    278.0, 278.0, 355.0, 556.0, 556.0, 889.0, 667.0, 191.0, 333.0, 333.0, 389.0, 584.0, 278.0,
    333.0, 278.0, 278.0, 556.0, 556.0, 556.0, 556.0, 556.0, 556.0, 556.0, 556.0, 556.0, 556.0,
    278.0, 278.0, 584.0, 584.0, 584.0, 556.0, 1015.0, 667.0, 667.0, 722.0, 722.0, 667.0, 611.0,
    778.0, 722.0, 278.0, 500.0, 667.0, 556.0, 833.0, 722.0, 778.0, 667.0, 778.0, 722.0, 667.0,
    611.0, 722.0, 667.0, 944.0, 667.0, 667.0, 611.0, 278.0, 278.0, 278.0, 469.0, 556.0, 333.0,
    556.0, 556.0, 500.0, 556.0, 556.0, 278.0, 556.0, 556.0, 222.0, 222.0, 500.0, 222.0, 833.0,
    556.0, 556.0, 556.0, 556.0, 333.0, 500.0, 278.0, 556.0, 500.0, 722.0, 500.0, 500.0, 500.0,
    334.0, 260.0, 334.0, 584.0,
];

/// Superscripts and primes the built-in unit formats use.
fn extra_advance(c: char) -> Option<f64> {
    Some(match c {
        '²' | '³' => 333.0,
        '′' => 191.0,
        '″' => 355.0,
        '−' => 584.0,
        '\u{a0}' => 278.0,
        _ => return None,
    })
}

/// From a line's top to its baseline, in em.
pub const HELVETICA_ASCENT: f64 = 0.962;

/// From one line's top to the next, in em.
pub const HELVETICA_LINE_HEIGHT: f64 = 1.169;

/// `helveticaAdvance(text)`: how far `text` advances, in em. A character
/// without a width here counts as wide as a digit.
pub fn helvetica_advance(text: &str) -> f64 {
    let mut advance = 0.0;
    for c in text.chars() {
        let code = c as u32;
        let ascii = code.checked_sub(32).and_then(|i| ADVANCES.get(i as usize).copied());
        advance += ascii.or_else(|| extra_advance(c)).unwrap_or(556.0);
    }
    advance / 1000.0
}
