//! Every annotation kind's record, in upstream's shape.
//!
//! G7. Ports the per-kind readers of
//! `services/src/features/annotations/internal/read/` (`readStyle.ts`,
//! `readShapeAnnotation.ts`, `readTextMarkupAnnotation.ts`,
//! `readVertexAnnotation.ts`, `readLineAnnotation.ts`,
//! `readInkAnnotation.ts`, `readTextAnnotation.ts`,
//! `readFreeTextAnnotation.ts`, `readStampAnnotation.ts`,
//! `readCaretAnnotation.ts`, `readRedactAnnotation.ts`,
//! `readLinkAnnotation.ts`, `readFileAttachmentAnnotation.ts`,
//! `readPopupAnnotation.ts`, `readUnsupportedAnnotation.ts`,
//! `readMeasurementFields.ts`) and `annotationReaderRegistry.ts`, read into
//! `core/src/annotation/kinds/`.
//!
//! A record is [`AnnotationRecordBase`] plus its kind's fields, flattened
//! into one object discriminated by `subtype`. Every field is present, `null`
//! when the file has none; colours are `"#rrggbb"`; quads name their corners.
//! The record is in the file's coordinates: the host boundary converts it to
//! page space ([`crate::page_space::annotations`]).
//!
//! A widget is not read here: widgets are the form's (`forms.list`,
//! [`super::record::read_widget_annotation`]).

use pdfium::{Annotation, Document};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::dictionary_read;
use super::file_attachment::{AttachmentFileInfo, read_attachment_info};
use super::link::{PdfLinkTarget, link_target_from_action_tree};
use super::record::{AnnotationRecordBase, HexColor, color_of, linked_ref};
use super::turn::{read_annotation_box, read_points_turn, upright_point};
use crate::error::EngineError;
use crate::features::actions::destination::read_destination;
use crate::features::annotation::intent::{
    CaretIntent, FreeTextIntent, InkIntent, LineIntent, PolygonIntent, PolylineIntent,
    StrikeoutIntent,
};
use crate::features::annotation::rich_text::{
    RichTextBody, RichTextDocument, RichTextParagraph, RichTextRun, free_text_font_for_face,
    read_rich_text,
};
use crate::features::annotation::state::{state_from_pdf, state_model_from_pdf};
use crate::features::annotation::subtype::{AnnotationSubtype, code};
use crate::features::annotation::vocabulary::{
    AnnotationBorderStyle, AnnotationColorType, DEFAULT_FILE_ATTACHMENT_ICON, DEFAULT_NOTE_ICON,
    DEFAULT_STANDARD_FONT, FileAttachmentIcon, LineEnding, LineEndings, NoteIcon, StandardFont,
    TextAlignment,
};
use crate::geom::{CalloutLine, LinePoints, NamedPdfQuad, PdfPoint, PdfRect, normalize_pdf_quad};

/// One annotation, as `annotations.list` answers it (`Annotation<C>`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AnnotationRecord {
    #[serde(flatten)]
    pub base: AnnotationRecordBase,
    #[serde(flatten)]
    pub kind: AnnotationRecordKind,
}

/// The kind's half of a record.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "subtype", rename_all = "kebab-case", rename_all_fields = "camelCase")]
pub enum AnnotationRecordKind {
    Highlight {
        #[serde(flatten)]
        fields: TextMarkupRecord,
    },
    Underline {
        #[serde(flatten)]
        fields: TextMarkupRecord,
    },
    Squiggly {
        #[serde(flatten)]
        fields: TextMarkupRecord,
    },
    Strikeout {
        #[serde(flatten)]
        fields: TextMarkupRecord,
        intent: Option<StrikeoutIntent>,
    },
    Square {
        #[serde(flatten)]
        fields: ShapeRecord,
    },
    Circle {
        #[serde(flatten)]
        fields: ShapeRecord,
    },
    Polygon {
        #[serde(flatten)]
        fields: VertexRecord,
        intent: Option<PolygonIntent>,
        cloudy_intensity: Option<f64>,
    },
    Polyline {
        #[serde(flatten)]
        fields: VertexRecord,
        intent: Option<PolylineIntent>,
        line_endings: LineEndings,
    },
    Line {
        #[serde(flatten)]
        fields: Box<LineRecord>,
    },
    Ink {
        #[serde(flatten)]
        fields: InkRecord,
    },
    Text {
        #[serde(flatten)]
        fields: TextRecord,
    },
    FreeText {
        #[serde(flatten)]
        fields: Box<FreeTextRecord>,
    },
    Stamp {
        #[serde(flatten)]
        fields: StampRecord,
    },
    Caret {
        #[serde(flatten)]
        fields: CaretRecord,
    },
    Redact {
        #[serde(flatten)]
        fields: RedactRecord,
    },
    Link {
        target: Option<PdfLinkTarget>,
    },
    FileAttachment {
        #[serde(flatten)]
        fields: FileAttachmentRecord,
    },
    Popup {
        /// `/Parent`: the annotation whose text it shows.
        parent: Option<crate::identity::AnnotationRef>,
        open: bool,
    },
    Unsupported {
        raw_subtype_code: i32,
        raw_subtype_name: Option<String>,
    },
}

/// The border every bordered kind reads (`BorderFields`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BorderRecord {
    pub stroke_width: f64,
    pub border_style: AnnotationBorderStyle,
    /// `None` for a solid border.
    pub dash_array: Option<Vec<f64>>,
}

/// `/C`, `/CA` and the border (`GeometryStyleFields`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeometryStyleRecord {
    pub color: HexColor,
    pub opacity: f64,
    #[serde(flatten)]
    pub border: BorderRecord,
}

/// [`GeometryStyleRecord`] and `/IC` (`FilledStyleFields`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilledStyleRecord {
    #[serde(flatten)]
    pub geometry: GeometryStyleRecord,
    pub interior_color: Option<HexColor>,
}

/// Highlight, underline, squiggly and strikeout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextMarkupRecord {
    pub color: HexColor,
    pub opacity: f64,
    pub quad_points: Vec<NamedPdfQuad>,
}

/// Square and circle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShapeRecord {
    #[serde(flatten)]
    pub style: FilledStyleRecord,
    #[serde(rename = "box")]
    pub content_box: PdfRect,
    pub rotation: Option<f64>,
    pub cloudy_intensity: Option<f64>,
}

/// Polygon and polyline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VertexRecord {
    pub measure: Option<Value>,
    pub caption_enabled: Option<bool>,
    pub caption_center: Option<PdfPoint>,
    #[serde(flatten)]
    pub style: FilledStyleRecord,
    pub vertices: Vec<PdfPoint>,
    pub rotation: Option<f64>,
}

/// `/CO`: a line caption's offset (`LineDimensionCaption.offset`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CaptionOffset {
    pub along: f64,
    pub perpendicular: f64,
}

/// `/LL`, `/LLE` and `/LLO` (`LineLeader`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LineLeader {
    pub length: f64,
    pub extension: f64,
    pub offset: f64,
}

/// Where a line's caption sits (`/CP`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CaptionPosition {
    Inline,
    Top,
}

/// Line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineRecord {
    pub measure: Option<Value>,
    pub intent: Option<LineIntent>,
    pub caption_enabled: Option<bool>,
    pub caption_position: CaptionPosition,
    pub caption_offset: Option<CaptionOffset>,
    pub leader: Option<LineLeader>,
    #[serde(flatten)]
    pub style: FilledStyleRecord,
    pub line_points: LinePoints,
    pub line_endings: LineEndings,
    pub rotation: Option<f64>,
}

/// Ink.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InkRecord {
    #[serde(flatten)]
    pub style: GeometryStyleRecord,
    pub intent: Option<InkIntent>,
    pub ink_list: Vec<Vec<PdfPoint>>,
    pub rotation: Option<f64>,
}

/// A sticky note.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextRecord {
    pub icon: NoteIcon,
    /// `/Open`, else its popup's.
    pub open: bool,
    pub color: HexColor,
    pub opacity: f64,
    pub state: Option<String>,
    pub state_model: Option<String>,
}

/// Where a free text's lines sit in its box (`VerticalAlignment`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VerticalAlignment {
    Top,
    Middle,
    Bottom,
}

impl VerticalAlignment {
    /// `verticalAlignmentFromCode`: our `/EMBD_Metadata
    /// /VerticalAlignment` number, 0 top, 1 middle, 2 bottom; anything else
    /// is top, as the generator draws it.
    pub fn from_code(code: Option<f64>) -> Self {
        match code {
            Some(1.0) => Self::Middle,
            Some(2.0) => Self::Bottom,
            _ => Self::Top,
        }
    }
}

/// `VERTICAL_ALIGNMENT_KEY`.
const VERTICAL_ALIGNMENT_KEY: &str = "VerticalAlignment";

/// Free text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FreeTextRecord {
    pub intent: FreeTextIntent,
    pub font_family: String,
    pub font_size: f64,
    /// The rich body's alignment, `justify` included, else `/Q`'s.
    pub text_align: String,
    pub vertical_align: VerticalAlignment,
    pub rich_text: RichTextDocument,
    /// `/DA`'s colour: the border's.
    pub color: HexColor,
    /// The rich body's colour: the text's.
    pub font_color: HexColor,
    /// `/C`: the box background.
    pub interior_color: Option<HexColor>,
    pub opacity: f64,
    #[serde(flatten)]
    pub border: BorderRecord,
    pub cloudy_intensity: Option<f64>,
    #[serde(rename = "box")]
    pub content_box: PdfRect,
    pub rotation: Option<f64>,
    pub callout_line: Option<CalloutLine>,
    pub line_ending: Option<LineEnding>,
}

/// How a stamp's drawing fills its box (`StampFit`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StampFit {
    Contain,
    Cover,
    Fill,
}

/// `KEY_APPEARANCE_FIT`.
const KEY_APPEARANCE_FIT: &str = "AppearanceFit";

/// Stamp.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StampRecord {
    /// `/Name`, verbatim: a standard label or a custom identifier.
    pub name: Option<String>,
    pub fit: Option<StampFit>,
    pub opacity: f64,
    #[serde(rename = "box")]
    pub content_box: PdfRect,
    pub rotation: Option<f64>,
}

/// Caret.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaretRecord {
    pub intent: Option<CaretIntent>,
    pub color: HexColor,
    pub opacity: f64,
    #[serde(rename = "box")]
    pub content_box: PdfRect,
    pub rotation: Option<f64>,
}

/// Redaction marking.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RedactRecord {
    pub quad_points: Vec<NamedPdfQuad>,
    pub color: HexColor,
    pub opacity: f64,
    pub interior_color: Option<HexColor>,
    pub overlay_text: Option<String>,
    pub repeat: bool,
    pub font_family: String,
    /// `/DA`'s size, `0` kept: a redaction label's auto-fit.
    pub font_size: f64,
    pub font_color: HexColor,
    pub text_align: TextAlignment,
}

/// File attachment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileAttachmentRecord {
    pub icon: FileAttachmentIcon,
    /// `None` when there is no readable `/FS`.
    pub file: Option<AttachmentFileInfo>,
    pub color: HexColor,
    pub opacity: f64,
}

// ── primitives ──────────────────────────────────────────────────────────────

const RED: (u8, u8, u8) = (255, 0, 0);
const YELLOW: (u8, u8, u8) = (255, 255, 0);
const BLACK: (u8, u8, u8) = (0, 0, 0);

/// `readAnnotColor`.
fn color(annotation: &Annotation<'_>, color_type: AnnotationColorType) -> Option<HexColor> {
    annotation.color(color_type as i32).map(color_of)
}

/// `/CA` as written (`readAnnotOpacity`), `None` when it can't be read.
fn raw_opacity(annotation: &Annotation<'_>) -> Option<f64> {
    annotation.opacity().map(super::turn::float32_decimal)
}

/// `/CA` clamped to 0..1, 1 when unreadable.
fn opacity(annotation: &Annotation<'_>) -> f64 {
    raw_opacity(annotation).map_or(1.0, |ca| ca.clamp(0.0, 1.0))
}

fn point(p: pdfium::PointF) -> PdfPoint {
    PdfPoint { x: f64::from(p.x), y: f64::from(p.y) }
}

fn points(list: Vec<pdfium::PointF>) -> Vec<PdfPoint> {
    list.into_iter().map(point).collect()
}

/// `readQuadPoints`: each entry's corners named by `normalizePdfQuad`.
fn quad_points(annotation: &Annotation<'_>) -> Result<Vec<NamedPdfQuad>, EngineError> {
    Ok(dictionary_read("QuadPoints", annotation.quad_points())?
        .into_iter()
        .map(|q| {
            normalize_pdf_quad([
                PdfPoint { x: f64::from(q.x1), y: f64::from(q.y1) },
                PdfPoint { x: f64::from(q.x2), y: f64::from(q.y2) },
                PdfPoint { x: f64::from(q.x3), y: f64::from(q.y3) },
                PdfPoint { x: f64::from(q.x4), y: f64::from(q.y4) },
            ])
        })
        .collect())
}

/// `/BE`: positive, else none (`readBorderEffect`).
fn cloudy_intensity(annotation: &Annotation<'_>) -> Option<f64> {
    super::read_cloudy_intensity(annotation)
}

/// `/IT` (`readIntent`).
fn intent(annotation: &Annotation<'_>) -> Result<Option<String>, EngineError> {
    dictionary_read("IT", annotation.intent())
}

/// `/LE`, `none` at both ends when absent (`readLineEndings`).
fn line_endings(annotation: &Annotation<'_>) -> LineEndings {
    annotation.line_endings().map_or(
        LineEndings { start: LineEnding::None, end: LineEnding::None },
        |(start, end)| LineEndings {
            start: LineEnding::from_code(start),
            end: LineEnding::from_code(end),
        },
    )
}

/// `readBorderFields`.
fn border(annotation: &Annotation<'_>) -> Result<BorderRecord, EngineError> {
    let style = annotation.border_style();
    let dash = dictionary_read("BS", annotation.border_dash_pattern())?;
    Ok(BorderRecord {
        stroke_width: f64::from(style.width),
        border_style: AnnotationBorderStyle::from_code(style.style_code),
        dash_array: (!dash.is_empty()).then(|| dash.into_iter().map(f64::from).collect()),
    })
}

/// `readGeometryStyleExtras`: red when there is no `/C`.
fn geometry_style(annotation: &Annotation<'_>) -> Result<GeometryStyleRecord, EngineError> {
    Ok(GeometryStyleRecord {
        color: color(annotation, AnnotationColorType::Color).unwrap_or_else(|| color_of(RED)),
        opacity: opacity(annotation),
        border: border(annotation)?,
    })
}

/// `readFilledStyleExtras`.
fn filled_style(annotation: &Annotation<'_>) -> Result<FilledStyleRecord, EngineError> {
    Ok(FilledStyleRecord {
        geometry: geometry_style(annotation)?,
        interior_color: color(annotation, AnnotationColorType::InteriorColor),
    })
}

/// `readAnnotationMeasure`.
fn measure(annotation: &Annotation<'_>) -> Result<Option<Value>, EngineError> {
    crate::features::measure::read_measure(annotation.measure())
}

/// `readShapeCaption`: `None` for a shape without our caption flag or a
/// manual center, so an imported shape keeps its appearance.
fn shape_caption(annotation: &Annotation<'_>) -> Option<(bool, Option<PdfPoint>)> {
    let caption = annotation.shape_caption()?;
    let present = annotation.embed_metadata_boolean("MeasurementCaption").ok().flatten().is_some();
    if !present && caption.center.is_none() {
        return None;
    }
    Some((caption.enabled, caption.center.map(point)))
}

fn has_key(annotation: &Annotation<'_>, key: &str) -> bool {
    annotation.has_key(key).unwrap_or(false)
}

/// The annotation's popup's `/Open`, which a note without its own shows.
fn popup_open(annotation: &Annotation<'_>) -> Result<Option<bool>, EngineError> {
    let Some(popup) = dictionary_read("Popup", annotation.linked_annotation("Popup"))? else {
        return Ok(None);
    };
    dictionary_read("Open", popup.boolean_value("Open"))
}

// ── the kinds ───────────────────────────────────────────────────────────────

fn read_text_markup(
    annotation: &Annotation<'_>,
    fallback: (u8, u8, u8),
) -> Result<TextMarkupRecord, EngineError> {
    Ok(TextMarkupRecord {
        color: color(annotation, AnnotationColorType::Color).unwrap_or_else(|| color_of(fallback)),
        opacity: opacity(annotation),
        quad_points: quad_points(annotation)?,
    })
}

fn read_shape(annotation: &Annotation<'_>) -> Result<ShapeRecord, EngineError> {
    let shape = read_annotation_box(annotation);
    Ok(ShapeRecord {
        style: filled_style(annotation)?,
        content_box: shape.content_box,
        rotation: shape.rotation,
        cloudy_intensity: cloudy_intensity(annotation),
    })
}

fn read_vertex(annotation: &Annotation<'_>) -> Result<VertexRecord, EngineError> {
    let drawn = points(dictionary_read("Vertices", annotation.vertices())?);
    let turn = read_points_turn(annotation, std::slice::from_ref(&drawn));
    let caption = shape_caption(annotation);
    Ok(VertexRecord {
        measure: measure(annotation)?,
        caption_enabled: caption.map(|(enabled, _)| enabled),
        caption_center: caption.and_then(|(_, center)| center).map(|c| upright_point(c, turn)),
        style: filled_style(annotation)?,
        vertices: drawn.into_iter().map(|p| upright_point(p, turn)).collect(),
        rotation: turn.map(|turn| turn.degrees),
    })
}

fn read_line(annotation: &Annotation<'_>) -> Result<LineRecord, EngineError> {
    let zero = PdfPoint { x: 0.0, y: 0.0 };
    let (start, end) =
        annotation.line().map_or((zero, zero), |(start, end)| (point(start), point(end)));
    let turn = read_points_turn(annotation, &[vec![start, end]]);
    let caption = ["Cap", "CP", "CO"]
        .iter()
        .any(|key| has_key(annotation, key))
        .then(|| annotation.line_caption())
        .flatten();
    let leader = ["LL", "LLE", "LLO"]
        .iter()
        .any(|key| has_key(annotation, key))
        .then(|| annotation.line_leader())
        .flatten();
    Ok(LineRecord {
        measure: measure(annotation)?,
        intent: LineIntent::from_name(intent(annotation)?.as_deref()),
        // An absent `/Cap` reads `null`, as on polygons and polylines; `/CP`
        // reads its ISO default.
        caption_enabled: has_key(annotation, "Cap")
            .then(|| caption.is_some_and(|caption| caption.enabled)),
        caption_position: match caption {
            Some(caption) if caption.position_code == 1 => CaptionPosition::Top,
            _ => CaptionPosition::Inline,
        },
        caption_offset: caption.filter(|_| has_key(annotation, "CO")).map(|caption| {
            CaptionOffset {
                along: f64::from(caption.along),
                perpendicular: f64::from(caption.perpendicular),
            }
        }),
        leader: leader.map(|(length, extension, offset)| LineLeader {
            length: f64::from(length),
            extension: f64::from(extension),
            offset: f64::from(offset),
        }),
        style: filled_style(annotation)?,
        line_points: LinePoints {
            start: upright_point(start, turn),
            end: upright_point(end, turn),
        },
        line_endings: line_endings(annotation),
        rotation: turn.map(|turn| turn.degrees),
    })
}

fn read_ink(annotation: &Annotation<'_>) -> Result<InkRecord, EngineError> {
    let drawn: Vec<Vec<PdfPoint>> = dictionary_read("InkList", annotation.ink_list())?
        .into_iter()
        .map(points)
        .filter(|stroke| !stroke.is_empty())
        .collect();
    let turn = read_points_turn(annotation, &drawn);
    Ok(InkRecord {
        style: geometry_style(annotation)?,
        intent: InkIntent::from_name(intent(annotation)?.as_deref()),
        ink_list: drawn
            .into_iter()
            .map(|stroke| stroke.into_iter().map(|p| upright_point(p, turn)).collect())
            .collect(),
        rotation: turn.map(|turn| turn.degrees),
    })
}

fn read_text(annotation: &Annotation<'_>) -> Result<TextRecord, EngineError> {
    let open = match dictionary_read("Open", annotation.boolean_value("Open"))? {
        Some(open) => Some(open),
        None => popup_open(annotation)?,
    };
    Ok(TextRecord {
        icon: dictionary_read("Name", annotation.name())?
            .and_then(|name| NoteIcon::from_pdf_name(&name))
            .unwrap_or(DEFAULT_NOTE_ICON),
        open: open.unwrap_or(false),
        color: color(annotation, AnnotationColorType::Color).unwrap_or_else(|| color_of(YELLOW)),
        opacity: opacity(annotation),
        state: dictionary_read("State", annotation.string_value("State"))?
            .map(|value| state_from_pdf(&value)),
        state_model: dictionary_read("StateModel", annotation.string_value("StateModel"))?
            .map(|value| state_model_from_pdf(&value)),
    })
}

/// `DEFAULT_FONT_SIZE` of free text and redaction labels.
const DEFAULT_FONT_SIZE: f64 = 12.0;

fn key_for_face(family: &str, weight: u32, italic: bool) -> Option<String> {
    crate::features::fonts::key_for_face(family, weight, italic)
}

fn read_free_text(
    annotation: &Annotation<'_>,
    contents: Option<&str>,
) -> Result<FreeTextRecord, EngineError> {
    let da = annotation.default_appearance();
    let color = da.map_or_else(|| color_of(BLACK), |da| color_of(da.color));
    let rich = read_rich_text(annotation);
    let font_family = match (&rich, da) {
        (Some(rich), _) => free_text_font_for_face(
            &rich.body.family,
            rich.body.weight,
            rich.body.italic,
            Some(&key_for_face),
        ),
        (None, Some(da)) => StandardFont::from_code(da.font_code).as_str().to_owned(),
        (None, None) => DEFAULT_STANDARD_FONT.as_str().to_owned(),
    };
    let font_size = rich
        .as_ref()
        .map(|rich| rich.body.size)
        .filter(|size| *size > 0.0)
        .or_else(|| da.map(|da| f64::from(da.font_size)).filter(|size| *size > 0.0))
        .unwrap_or(DEFAULT_FONT_SIZE);
    // The rich body's alignment is what the appearance paints; a plain box's
    // synthesised body carries /Q.
    let text_align = match &rich {
        Some(rich) => rich.body.align.clone(),
        None => TextAlignment::from_code(annotation.text_alignment()).as_str().to_owned(),
    };
    let rich_text = rich.unwrap_or_else(|| synthesised(font_size, &color, contents));
    let font_color = rich_text.body.color.to_lowercase();
    let vertical = annotation.embed_metadata_number(VERTICAL_ALIGNMENT_KEY).ok().flatten();
    let callout = points(dictionary_read("CL", annotation.callout_line())?);
    let callout_line = match callout.len() {
        2 => Some(CalloutLine::Straight([callout[0], callout[1]])),
        3 => Some(CalloutLine::Kneed([callout[0], callout[1], callout[2]])),
        _ => None,
    };
    let leader_end = line_endings(annotation).end;
    let shape = read_annotation_box(annotation);
    Ok(FreeTextRecord {
        intent: FreeTextIntent::from_name(intent(annotation)?.as_deref()),
        font_family,
        font_size,
        text_align,
        vertical_align: VerticalAlignment::from_code(vertical.map(f64::from)),
        rich_text,
        color,
        font_color,
        interior_color: self::color(annotation, AnnotationColorType::Color),
        opacity: opacity(annotation),
        border: border(annotation)?,
        cloudy_intensity: cloudy_intensity(annotation),
        content_box: shape.content_box,
        rotation: shape.rotation,
        line_ending: (callout_line.is_some() && leader_end != LineEnding::None)
            .then_some(leader_end),
        callout_line,
    })
}

/// The one-run document a free text with no rich text reads as.
fn synthesised(size: f64, color: &str, contents: Option<&str>) -> RichTextDocument {
    RichTextDocument {
        body: RichTextBody {
            family: "Helvetica".into(),
            weight: 400,
            italic: false,
            size,
            color: color.to_owned(),
            decoration: Vec::new(),
            script: "normal".into(),
            letter_spacing: 0.0,
            horizontal_scale: 1.0,
            align: "left".into(),
            dir: "ltr".into(),
            rest: serde_json::Map::new(),
        },
        paragraphs: vec![RichTextParagraph {
            runs: vec![RichTextRun { text: contents.unwrap_or_default().to_owned(), style: None }],
            props: serde_json::Map::new(),
        }],
    }
}

fn read_stamp(annotation: &Annotation<'_>) -> Result<StampRecord, EngineError> {
    let fit = if annotation.has_embed_metadata() {
        dictionary_read(KEY_APPEARANCE_FIT, annotation.embed_metadata_string(KEY_APPEARANCE_FIT))?
    } else {
        None
    };
    let shape = read_annotation_box(annotation);
    Ok(StampRecord {
        name: dictionary_read("Name", annotation.name())?,
        fit: match fit.as_deref() {
            Some("contain") => Some(StampFit::Contain),
            Some("cover") => Some(StampFit::Cover),
            Some("fill") => Some(StampFit::Fill),
            _ => None,
        },
        opacity: raw_opacity(annotation).unwrap_or(1.0),
        content_box: shape.content_box,
        rotation: shape.rotation,
    })
}

fn read_caret(annotation: &Annotation<'_>) -> Result<CaretRecord, EngineError> {
    let shape = read_annotation_box(annotation);
    Ok(CaretRecord {
        intent: CaretIntent::from_name(intent(annotation)?.as_deref()),
        color: color(annotation, AnnotationColorType::Color).unwrap_or_else(|| color_of(RED)),
        opacity: opacity(annotation),
        content_box: shape.content_box,
        rotation: shape.rotation,
    })
}

fn read_redact(annotation: &Annotation<'_>) -> Result<RedactRecord, EngineError> {
    let da = annotation.default_appearance();
    // The face the /DA names, resolved by identity as free text does.
    let face = da.and_then(|_| read_rich_text(annotation)).map(|rich| rich.body);
    let font_family = match (&face, da) {
        (Some(face), _) if !face.family.is_empty() => {
            free_text_font_for_face(&face.family, face.weight, face.italic, Some(&key_for_face))
        }
        (_, Some(da)) => StandardFont::from_code(da.font_code).as_str().to_owned(),
        (_, None) => DEFAULT_STANDARD_FONT.as_str().to_owned(),
    };
    Ok(RedactRecord {
        quad_points: quad_points(annotation)?,
        color: color(annotation, AnnotationColorType::Color).unwrap_or_else(|| color_of(RED)),
        opacity: opacity(annotation),
        interior_color: color(annotation, AnnotationColorType::InteriorColor),
        overlay_text: dictionary_read("OverlayText", annotation.overlay_text())?,
        repeat: annotation.overlay_text_repeat(),
        font_family,
        font_size: da.map_or(DEFAULT_FONT_SIZE, |da| f64::from(da.font_size)),
        font_color: da.map_or_else(|| color_of(BLACK), |da| color_of(da.color)),
        text_align: TextAlignment::from_code(annotation.text_alignment()),
    })
}

/// `readLinkTarget`: the activate action's target, else the direct
/// `/Dest`; `None` for a dead link.
fn read_link_target(
    document: &Document,
    annotation: &Annotation<'_>,
    base: &AnnotationRecordBase,
) -> Option<PdfLinkTarget> {
    if let Some(tree) = base.actions.as_ref().and_then(|actions| actions.activate.as_ref()) {
        return Some(link_target_from_action_tree(tree));
    }
    document.link_destination(annotation).map(|raw| {
        read_destination(&raw)
            .map_or(PdfLinkTarget::Unsupported, |destination| PdfLinkTarget::Goto { destination })
    })
}

fn read_file_attachment(annotation: &Annotation<'_>) -> Result<FileAttachmentRecord, EngineError> {
    let file = match annotation.file_attachment() {
        Some(attachment) => Some(read_attachment_info(&attachment)?),
        None => None,
    };
    Ok(FileAttachmentRecord {
        icon: dictionary_read("Name", annotation.name())?
            .and_then(|name| FileAttachmentIcon::from_pdf_name(&name))
            .unwrap_or(DEFAULT_FILE_ATTACHMENT_ICON),
        file,
        color: color(annotation, AnnotationColorType::Color).unwrap_or_else(|| color_of(YELLOW)),
        opacity: opacity(annotation),
    })
}

/// Read a kind's half (`pickReader` and the reader it picks). A subtype the
/// catalog doesn't name reads as `unsupported`, with its code and name.
/// A widget is the form's to read, so it reads as `unsupported` here too:
/// no list built from this ever holds one.
pub fn read_annotation_record_kind(
    document: &Document,
    annotation: &Annotation<'_>,
    base: &AnnotationRecordBase,
    raw_subtype_code: i32,
) -> Result<AnnotationRecordKind, EngineError> {
    use AnnotationRecordKind as K;
    Ok(match AnnotationSubtype::from_code(raw_subtype_code) {
        AnnotationSubtype::Highlight => {
            K::Highlight { fields: read_text_markup(annotation, YELLOW)? }
        }
        AnnotationSubtype::Underline => {
            K::Underline { fields: read_text_markup(annotation, BLACK)? }
        }
        AnnotationSubtype::Squiggly => K::Squiggly { fields: read_text_markup(annotation, BLACK)? },
        AnnotationSubtype::Strikeout => K::Strikeout {
            fields: read_text_markup(annotation, BLACK)?,
            intent: StrikeoutIntent::from_name(intent(annotation)?.as_deref()),
        },
        AnnotationSubtype::Square => K::Square { fields: read_shape(annotation)? },
        AnnotationSubtype::Circle => K::Circle { fields: read_shape(annotation)? },
        AnnotationSubtype::Polygon => K::Polygon {
            fields: read_vertex(annotation)?,
            intent: PolygonIntent::from_name(intent(annotation)?.as_deref()),
            cloudy_intensity: cloudy_intensity(annotation),
        },
        AnnotationSubtype::Polyline => K::Polyline {
            fields: read_vertex(annotation)?,
            intent: PolylineIntent::from_name(intent(annotation)?.as_deref()),
            line_endings: line_endings(annotation),
        },
        AnnotationSubtype::Line => K::Line { fields: Box::new(read_line(annotation)?) },
        AnnotationSubtype::Ink => K::Ink { fields: read_ink(annotation)? },
        AnnotationSubtype::Text => K::Text { fields: read_text(annotation)? },
        AnnotationSubtype::FreeText => {
            K::FreeText { fields: Box::new(read_free_text(annotation, base.contents.as_deref())?) }
        }
        AnnotationSubtype::Stamp => K::Stamp { fields: read_stamp(annotation)? },
        AnnotationSubtype::Caret => K::Caret { fields: read_caret(annotation)? },
        AnnotationSubtype::Redact => K::Redact { fields: read_redact(annotation)? },
        AnnotationSubtype::Link => K::Link { target: read_link_target(document, annotation, base) },
        AnnotationSubtype::FileAttachment => {
            K::FileAttachment { fields: read_file_attachment(annotation)? }
        }
        AnnotationSubtype::Popup => K::Popup {
            parent: linked_ref(document, annotation, "Parent", base.page)?,
            open: dictionary_read("Open", annotation.boolean_value("Open"))?.unwrap_or(false),
        },
        AnnotationSubtype::Widget | AnnotationSubtype::Unsupported => K::Unsupported {
            raw_subtype_code,
            raw_subtype_name: dictionary_read("Subtype", annotation.string_value("Subtype"))?,
        },
    })
}

/// Whether a subtype code is the form's (`familyOfCode`): a widget.
pub fn is_widget_code(subtype_code: i32) -> bool {
    subtype_code == code::WIDGET
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_vertical_alignment_is_top_unless_it_says_otherwise() {
        assert_eq!(VerticalAlignment::from_code(None), VerticalAlignment::Top);
        assert_eq!(VerticalAlignment::from_code(Some(1.0)), VerticalAlignment::Middle);
        assert_eq!(VerticalAlignment::from_code(Some(2.0)), VerticalAlignment::Bottom);
        assert_eq!(VerticalAlignment::from_code(Some(0.5)), VerticalAlignment::Top);
        assert_eq!(VerticalAlignment::from_code(Some(3.0)), VerticalAlignment::Top);
    }

    #[test]
    fn the_subtype_is_the_tag_and_kebab_case() {
        let kind = AnnotationRecordKind::Popup { parent: None, open: true };
        assert_eq!(
            serde_json::to_value(&kind).unwrap(),
            serde_json::json!({ "subtype": "popup", "parent": null, "open": true })
        );
        let unsupported = AnnotationRecordKind::Unsupported {
            raw_subtype_code: 18,
            raw_subtype_name: Some("Sound".into()),
        };
        assert_eq!(
            serde_json::to_value(&unsupported).unwrap(),
            serde_json::json!({ "subtype": "unsupported", "rawSubtypeCode": 18,
                "rawSubtypeName": "Sound" })
        );
    }
}
