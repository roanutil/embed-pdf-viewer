//! `richtext.ts`: the rich text run algebra over a FreeText's rich document
//! (engine `RichTextDocumentInput`). Offsets count UTF-16 code units of the
//! plain projection: paragraphs joined by `\r`, runs concatenated.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::js::utf16_len;

/// `RichTextStyleDelta = Partial<RichTextRunStyle>`: the properties a run
/// overrides.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RichTextStyleDelta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decoration: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub letter_spacing: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub horizontal_scale: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unknown: Option<String>,
}

/// `keyof RichTextRunStyle`, the delta keys in declaration order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RichTextStyleKey {
    Family,
    Weight,
    Italic,
    Size,
    Color,
    Decoration,
    Script,
    LetterSpacing,
    HorizontalScale,
    Unknown,
}

impl RichTextStyleDelta {
    fn is_empty(&self) -> bool {
        *self == RichTextStyleDelta::default()
    }

    /// `{ ...self, ...over }`.
    fn spread(&self, over: &RichTextStyleDelta) -> RichTextStyleDelta {
        RichTextStyleDelta {
            family: over.family.clone().or_else(|| self.family.clone()),
            weight: over.weight.or(self.weight),
            italic: over.italic.or(self.italic),
            size: over.size.or(self.size),
            color: over.color.clone().or_else(|| self.color.clone()),
            decoration: over.decoration.clone().or_else(|| self.decoration.clone()),
            script: over.script.clone().or_else(|| self.script.clone()),
            letter_spacing: over.letter_spacing.or(self.letter_spacing),
            horizontal_scale: over.horizontal_scale.or(self.horizontal_scale),
            unknown: over.unknown.clone().or_else(|| self.unknown.clone()),
        }
    }

    /// `delete style[key]`.
    fn clear(&mut self, key: RichTextStyleKey) {
        match key {
            RichTextStyleKey::Family => self.family = None,
            RichTextStyleKey::Weight => self.weight = None,
            RichTextStyleKey::Italic => self.italic = None,
            RichTextStyleKey::Size => self.size = None,
            RichTextStyleKey::Color => self.color = None,
            RichTextStyleKey::Decoration => self.decoration = None,
            RichTextStyleKey::Script => self.script = None,
            RichTextStyleKey::LetterSpacing => self.letter_spacing = None,
            RichTextStyleKey::HorizontalScale => self.horizontal_scale = None,
            RichTextStyleKey::Unknown => self.unknown = None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RichTextRun {
    /// May contain `\r`: a hard break inside the paragraph.
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<RichTextStyleDelta>,
}

/// A paragraph: its runs, and the paragraph properties it overrides
/// (`align`, `dir`, `lineHeight`, `margins`, `textIndent`, `unknown`),
/// carried verbatim.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RichTextParagraph {
    pub runs: Vec<RichTextRun>,
    #[serde(flatten)]
    pub props: Map<String, Value>,
}

/// `RichTextDocumentInput`: a partial body and the paragraphs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RichTextDocumentInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<Map<String, Value>>,
    pub paragraphs: Vec<RichTextParagraph>,
}

/// A range of the plain projection, end exclusive.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RichTextRange {
    pub start: f64,
    pub end: f64,
}

/// Where an offset falls.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct OffsetLocation {
    pub paragraph: usize,
    pub offset: f64,
}

fn text_len(s: &str) -> f64 {
    utf16_len(s) as f64
}

fn paragraph_len(p: &RichTextParagraph) -> f64 {
    p.runs.iter().fold(0.0, |n, r| n + text_len(&r.text))
}

/// `s.slice(from, to)` in UTF-16 units. A cut inside a surrogate pair
/// becomes U+FFFD, where the TypeScript keeps the lone surrogate.
fn utf16_slice(s: &str, from: usize, to: usize) -> String {
    let units: Vec<u16> = s.encode_utf16().collect();
    let to = to.min(units.len());
    let from = from.min(to);
    String::from_utf16_lossy(&units[from..to])
}

/// The length of the plain projection.
pub fn rich_text_length(paragraphs: &[RichTextParagraph]) -> f64 {
    let mut n = 0.0;
    for (i, p) in paragraphs.iter().enumerate() {
        if i > 0 {
            n += 1.0;
        }
        n += paragraph_len(p);
    }
    n
}

fn same_decoration(a: Option<&Vec<String>>, b: Option<&Vec<String>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) if a.len() == b.len() => {
            let mut sa = a.clone();
            let mut sb = b.clone();
            sa.sort();
            sb.sort();
            sa == sb
        }
        _ => false,
    }
}

/// Two deltas override the same properties with the same values.
pub fn same_style_delta(a: Option<&RichTextStyleDelta>, b: Option<&RichTextStyleDelta>) -> bool {
    let empty = RichTextStyleDelta::default();
    let da = a.unwrap_or(&empty);
    let db = b.unwrap_or(&empty);
    da.family == db.family
        && da.weight == db.weight
        && da.italic == db.italic
        && da.size == db.size
        && da.color == db.color
        && same_decoration(da.decoration.as_ref(), db.decoration.as_ref())
        && da.script == db.script
        && da.letter_spacing == db.letter_spacing
        && da.horizontal_scale == db.horizontal_scale
        && da.unknown == db.unknown
}

/// The delta with nothing set as `None`.
fn clean_delta(delta: Option<&RichTextStyleDelta>) -> Option<RichTextStyleDelta> {
    delta.filter(|d| !d.is_empty()).cloned()
}

/// Merge adjacent runs whose deltas agree, drop empty runs and empty deltas.
/// A paragraph with no text keeps one empty run for the caret.
pub fn normalize_runs(paragraphs: &[RichTextParagraph]) -> Vec<RichTextParagraph> {
    paragraphs
        .iter()
        .map(|p| {
            let mut runs: Vec<RichTextRun> = Vec::new();
            for run in &p.runs {
                if run.text.is_empty() {
                    continue;
                }
                let style = clean_delta(run.style.as_ref());
                match runs.last_mut() {
                    Some(last) if same_style_delta(last.style.as_ref(), style.as_ref()) => {
                        last.text.push_str(&run.text);
                    }
                    _ => runs.push(RichTextRun { text: run.text.clone(), style }),
                }
            }
            if runs.is_empty() {
                runs.push(RichTextRun { text: String::new(), style: None });
            }
            RichTextParagraph { runs, props: p.props.clone() }
        })
        .collect()
}

/// `normalizeRuns(doc)` for a whole document.
pub fn normalize_doc(doc: &RichTextDocumentInput) -> RichTextDocumentInput {
    RichTextDocumentInput { body: doc.body.clone(), paragraphs: normalize_runs(&doc.paragraphs) }
}

/// The paragraph and local offset of a flat offset; the separator between
/// two paragraphs belongs to the end of the earlier one.
pub fn locate_offset(paragraphs: &[RichTextParagraph], offset: f64) -> OffsetLocation {
    let mut pos = 0.0;
    for (i, p) in paragraphs.iter().enumerate() {
        let length = paragraph_len(p);
        if offset <= pos + length || i == paragraphs.len() - 1 {
            return OffsetLocation {
                paragraph: i,
                offset: crate::js::max(0.0, crate::js::min(length, offset - pos)),
            };
        }
        pos += length + 1.0;
    }
    OffsetLocation { paragraph: 0, offset: 0.0 }
}

/// The paragraph with a run boundary at a local offset.
pub fn split_runs_at(paragraph: &RichTextParagraph, offset: f64) -> RichTextParagraph {
    let mut runs = Vec::new();
    let mut pos = 0.0;
    for run in &paragraph.runs {
        let end = pos + text_len(&run.text);
        if offset > pos && offset < end {
            let cut = (offset - pos) as usize;
            let len = utf16_len(&run.text);
            runs.push(RichTextRun { text: utf16_slice(&run.text, 0, cut), ..run.clone() });
            runs.push(RichTextRun { text: utf16_slice(&run.text, cut, len), ..run.clone() });
        } else {
            runs.push(run.clone());
        }
        pos = end;
    }
    RichTextParagraph { runs, props: paragraph.props.clone() }
}

/// Apply a delta to every character of a range (`clear` names properties to
/// stop overriding), then normalise. An empty range changes nothing.
pub fn apply_style_to_range(
    paragraphs: &[RichTextParagraph],
    range: RichTextRange,
    delta: &RichTextStyleDelta,
    clear: &[RichTextStyleKey],
) -> Option<Vec<RichTextParagraph>> {
    let start = crate::js::max(0.0, crate::js::min(range.start, range.end));
    let end = crate::js::max(range.start, range.end);
    if end <= start {
        return None;
    }
    let from = locate_offset(paragraphs, start);
    let to = locate_offset(paragraphs, end);
    let styled: Vec<RichTextParagraph> = paragraphs
        .iter()
        .enumerate()
        .map(|(index, paragraph)| {
            if index < from.paragraph || index > to.paragraph {
                return paragraph.clone();
            }
            let local_start = if index == from.paragraph { from.offset } else { 0.0 };
            let local_end =
                if index == to.paragraph { to.offset } else { paragraph_len(paragraph) };
            if local_end <= local_start {
                return paragraph.clone();
            }
            let split = split_runs_at(&split_runs_at(paragraph, local_start), local_end);
            let mut pos = 0.0;
            let runs = split
                .runs
                .iter()
                .map(|run| {
                    let run_start = pos;
                    pos += text_len(&run.text);
                    if run_start < local_start || run_start >= local_end || run.text.is_empty() {
                        return run.clone();
                    }
                    let mut style = run.style.clone().unwrap_or_default().spread(delta);
                    for key in clear {
                        style.clear(*key);
                    }
                    RichTextRun { style: Some(style), ..run.clone() }
                })
                .collect();
            RichTextParagraph { runs, props: split.props }
        })
        .collect();
    Some(normalize_runs(&styled))
}

/// The delta in force at a caret: the run the offset falls in, or at a run
/// boundary the run before it. `None` for no paragraph.
pub fn style_at(paragraphs: &[RichTextParagraph], offset: f64) -> Option<RichTextStyleDelta> {
    let at = locate_offset(paragraphs, offset);
    let paragraph = paragraphs.get(at.paragraph)?;
    let mut pos = 0.0;
    let mut previous: Option<&RichTextRun> = None;
    for run in &paragraph.runs {
        let end = pos + text_len(&run.text);
        if at.offset > pos && at.offset <= end {
            return Some(run.style.clone().unwrap_or_default());
        }
        if at.offset == pos && pos == 0.0 {
            return Some(run.style.clone().unwrap_or_default());
        }
        previous = Some(run);
        pos = end;
    }
    Some(previous.and_then(|r| r.style.clone()).unwrap_or_default())
}

/// Every run overlapping the range satisfies `predicate`; an empty range
/// tests the caret's style.
pub fn range_has_style(
    paragraphs: &[RichTextParagraph],
    range: RichTextRange,
    predicate: &dyn Fn(&RichTextStyleDelta) -> bool,
) -> bool {
    let start = crate::js::min(range.start, range.end);
    let end = crate::js::max(range.start, range.end);
    if end <= start {
        return predicate(&style_at(paragraphs, start).unwrap_or_default());
    }
    let from = locate_offset(paragraphs, start);
    let to = locate_offset(paragraphs, end);
    let mut any = false;
    let span = to.paragraph.saturating_sub(from.paragraph) + 1;
    // `paragraphs[index]!` reads past the end of an empty document and
    // throws; here the span is empty.
    for (index, paragraph) in paragraphs.iter().enumerate().skip(from.paragraph).take(span) {
        let local_start = if index == from.paragraph { from.offset } else { 0.0 };
        let local_end = if index == to.paragraph { to.offset } else { paragraph_len(paragraph) };
        let mut pos = 0.0;
        for run in &paragraph.runs {
            let run_end = pos + text_len(&run.text);
            if run_end > local_start && pos < local_end && !run.text.is_empty() {
                any = true;
                if !predicate(&run.style.clone().unwrap_or_default()) {
                    return false;
                }
            }
            pos = run_end;
        }
    }
    any
}

/// Plain paragraphs from text: one per line break, one unstyled run each.
pub fn paragraphs_from_plain_text(text: &str) -> Vec<RichTextParagraph> {
    text.replace("\r\n", "\n")
        .split(['\r', '\n'])
        .map(|line| RichTextParagraph {
            runs: vec![RichTextRun { text: line.into(), style: None }],
            props: Map::new(),
        })
        .collect()
}

/// The plain projection.
pub fn plain_text_of(paragraphs: &[RichTextParagraph]) -> String {
    paragraphs
        .iter()
        .map(|p| p.runs.iter().map(|r| r.text.as_str()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\r")
}

const PARAGRAPH_PROPS: [&str; 6] =
    ["align", "dir", "lineHeight", "margins", "textIndent", "unknown"];

/// Nothing overrides the body: no run delta, no paragraph property.
pub fn is_plain_rich_text(doc: &RichTextDocumentInput) -> bool {
    if doc.body.as_ref().is_some_and(|b| !b.is_empty()) {
        return false;
    }
    doc.paragraphs.iter().all(|p| {
        PARAGRAPH_PROPS.iter().all(|k| !p.props.contains_key(*k))
            && p.runs.iter().all(|r| clean_delta(r.style.as_ref()).is_none())
    })
}
