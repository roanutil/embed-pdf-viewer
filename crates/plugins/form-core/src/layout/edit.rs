//! `layout/edit.ts`: laid-out text as the generator writes it
//! (`GenerateEditAP`). A run per line, each starting where the line's first
//! character starts, moved by `offset` (the vertical centring of a one-line
//! field). A comb writes each character on its own, at its cell.

use super::TextLayout;
use crate::TextRun;
use crate::fround;

pub(crate) fn text_runs(layout: &TextLayout, offset: (f64, f64), continuous: bool) -> Vec<TextRun> {
    let (dx, dy) = offset;
    let mut runs = Vec::new();
    for line in &layout.lines {
        let Some(first) = line.words.first() else { continue };
        if continuous {
            let codes: Vec<u16> = line.words.iter().map(|word| word.code).collect();
            runs.push(TextRun {
                x: fround(first.x + dx),
                y: fround(first.y + dy),
                text: String::from_utf16_lossy(&codes),
                advances: line.words.iter().map(|word| word.width).collect(),
            });
        } else {
            for word in &line.words {
                runs.push(TextRun {
                    x: fround(word.x + dx),
                    y: fround(word.y + dy),
                    text: String::from_utf16_lossy(&[word.code]),
                    advances: vec![word.width],
                });
            }
        }
    }
    runs
}
