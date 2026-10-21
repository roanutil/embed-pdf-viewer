//! What a create writes: the draft checked against its kind and stated
//! whole.
//!
//! G11. Ports `services/src/features/annotations/internal/mutations/
//! prepareCreate.ts` and the pieces of `core/src/annotation/` it runs:
//! `resolve/resolveAnnotationDraft.ts` (`pdfPrepareAnnotationDraft`,
//! `pdfDeriveAnnotationDraft`), `checkWrite.ts` (`assertAnnotationDraft`),
//! `resolve/box.ts` (`boxDraftFollows`), `resolve/text.ts`,
//! `resolve/freeText.ts` (`freeTextDraftFollows`), `resolve/redact.ts`,
//! `resources.ts` (`assertAnnotationResources`) and the kinds' create
//! schemas (`kinds/*/declaration.ts`, `kinds/shared-fields.ts`).
//!
//! A draft arrives in the file's coordinates (the host's boundary converted
//! it) as the JSON object the caller sent. It is prepared (its names checked
//! against its kind, a box kind placed by its `rect` given its `box`), then
//! each field is checked against its kind's create schema, then what follows
//! from it is stated. A check only accepts or refuses: a field is never
//! added, dropped or changed by it, so what is written is what a viewer
//! predicts (`resolveAnnotationDraft`).
//!
//! Upstream's schemas are zod. Here each data field is read into its own
//! type, one at a time, so a refusal names the field as zod's first issue
//! does (`invalidWrite`): `InvalidArg`, `details.field`.

use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use super::context::AnnotationWriteContext;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::actions::destination::PdfDestination;
use crate::features::annotation::intent::{
    CaretIntent, FreeTextIntent, InkIntent, StrikeoutIntent,
};
use crate::features::annotation::rich_text::{
    RichTextDocumentInput, RichTextParagraph, face_for_free_text_font, paragraphs_from_plain_text,
    plain_text,
};
use crate::features::annotation::subtype::AnnotationSubtype;
use crate::features::annotation::vocabulary::{
    AnnotationBorderStyle, AnnotationReplyType, BlendMode, FileAttachmentIcon, LineEnding,
    LineEndings, NoteIcon, TextAlignment,
};
use crate::geom::{CalloutLine, InkList, LinePoints, NamedPdfQuad, PdfPoint, PdfRect};
use crate::identity::AnnotationRef;

/// Who owns a declared field (`FieldOwner`): only `data` fields are the
/// caller's to write. The rest are accepted so a read can be sent back as a
/// create, and the engine decides what happens to them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldOwner {
    Data,
    Attribution,
    Engine,
    Preserved,
}

use FieldOwner::{Attribution as A, Data as D, Engine as E, Preserved as P};

/// The fields every kind declares, before its own (`annotationBaseFields`).
/// `rect` is listed per kind: it is the engine's on a kind that works it
/// out from a drawing.
const BASE_FIELDS: &[(&str, FieldOwner)] = &[
    ("ref", E),
    ("page", E),
    ("hasAppearance", E),
    ("appearanceState", E),
    ("nm", D),
    ("contents", D),
    ("subject", D),
    ("blendMode", D),
    ("invisible", D),
    ("hidden", D),
    ("print", D),
    ("noZoom", D),
    ("noRotate", D),
    ("noView", D),
    ("readOnly", D),
    ("locked", D),
    ("toggleNoView", D),
    ("lockedContents", D),
    ("reply", D),
    ("popup", E),
    ("groupId", D),
    ("author", A),
    ("createdAt", A),
    ("modifiedAt", A),
    ("userId", A),
    ("createdBy", A),
    ("modifiedBy", A),
    ("importedBy", E),
];

const MARKUP: &[(&str, FieldOwner)] =
    &[("rect", E), ("actions", P), ("color", D), ("opacity", D), ("quadPoints", D)];
const SHAPE: &[(&str, FieldOwner)] = &[
    ("rect", E),
    ("actions", P),
    ("color", D),
    ("opacity", D),
    ("strokeWidth", D),
    ("borderStyle", D),
    ("dashArray", D),
    ("interiorColor", D),
    ("box", D),
    ("rotation", D),
    ("cloudyIntensity", D),
];
/// Each kind's own fields, after [`BASE_FIELDS`], with their owners: the
/// `owners` of `kind-facts.generated.ts`. `None` for a subtype the engine
/// has no kind for.
pub fn kind_fields(subtype: AnnotationSubtype) -> &'static [(&'static str, FieldOwner)] {
    use AnnotationSubtype as S;
    match subtype {
        S::Highlight | S::Underline | S::Squiggly => MARKUP,
        S::Strikeout => &[
            ("rect", E),
            ("actions", P),
            ("color", D),
            ("opacity", D),
            ("quadPoints", D),
            ("intent", D),
        ],
        S::Square | S::Circle => SHAPE,
        S::Polygon => &[
            ("rect", E),
            ("actions", P),
            ("color", D),
            ("opacity", D),
            ("strokeWidth", D),
            ("borderStyle", D),
            ("dashArray", D),
            ("interiorColor", D),
            ("vertices", D),
            ("rotation", D),
            ("captionEnabled", D),
            ("captionCenter", D),
            ("intent", D),
            ("measure", D),
            ("cloudyIntensity", D),
        ],
        S::Polyline => &[
            ("rect", E),
            ("actions", P),
            ("color", D),
            ("opacity", D),
            ("strokeWidth", D),
            ("borderStyle", D),
            ("dashArray", D),
            ("interiorColor", D),
            ("vertices", D),
            ("rotation", D),
            ("captionEnabled", D),
            ("captionCenter", D),
            ("intent", D),
            ("measure", D),
            ("lineEndings", D),
        ],
        S::Line => &[
            ("rect", E),
            ("actions", P),
            ("color", D),
            ("opacity", D),
            ("strokeWidth", D),
            ("borderStyle", D),
            ("dashArray", D),
            ("interiorColor", D),
            ("linePoints", D),
            ("lineEndings", D),
            ("rotation", D),
            ("intent", D),
            ("measure", D),
            ("captionEnabled", D),
            ("captionPosition", D),
            ("captionOffset", D),
            ("leader", D),
        ],
        S::Link => &[("rect", D), ("actions", P), ("target", D)],
        S::Ink => &[
            ("rect", E),
            ("actions", P),
            ("color", D),
            ("opacity", D),
            ("strokeWidth", D),
            ("borderStyle", D),
            ("dashArray", D),
            ("inkList", D),
            ("intent", D),
            ("rotation", D),
        ],
        S::FreeText => &[
            ("rect", E),
            ("actions", P),
            ("box", D),
            ("rotation", D),
            ("intent", D),
            ("fontFamily", D),
            ("fontSize", D),
            ("textAlign", D),
            ("verticalAlign", D),
            ("richText", D),
            ("color", D),
            ("fontColor", D),
            ("interiorColor", D),
            ("opacity", D),
            ("strokeWidth", D),
            ("borderStyle", D),
            ("dashArray", D),
            ("cloudyIntensity", D),
            ("calloutLine", D),
            ("lineEnding", D),
        ],
        S::Caret => &[
            ("rect", E),
            ("actions", P),
            ("color", D),
            ("opacity", D),
            ("box", D),
            ("rotation", D),
            ("intent", D),
        ],
        S::Text => &[
            ("rect", D),
            ("actions", P),
            ("color", D),
            ("opacity", D),
            ("icon", D),
            ("open", D),
            ("state", D),
            ("stateModel", D),
        ],
        S::Stamp => &[
            ("rect", E),
            ("actions", P),
            ("box", D),
            ("rotation", D),
            ("name", D),
            ("fit", D),
            ("opacity", D),
        ],
        S::FileAttachment => {
            &[("rect", D), ("actions", P), ("color", D), ("opacity", D), ("icon", D), ("file", D)]
        }
        S::Widget => &[
            ("rect", E),
            ("actions", D),
            ("box", D),
            ("rotation", D),
            ("color", D),
            ("interiorColor", D),
            ("strokeWidth", D),
            ("borderStyle", D),
            ("dashArray", D),
            ("fontFamily", D),
            ("fontSize", D),
            ("fontColor", D),
            ("textAlign", D),
            ("caption", D),
            ("checkStyle", D),
            ("field", E),
            ("fieldFamily", E),
        ],
        S::Redact => &[
            ("rect", D),
            ("actions", P),
            ("color", D),
            ("opacity", D),
            ("quadPoints", D),
            ("interiorColor", D),
            ("overlayText", D),
            ("repeat", D),
            ("fontFamily", D),
            ("fontSize", D),
            ("fontColor", D),
            ("textAlign", D),
        ],
        S::Popup => &[("rect", D), ("actions", P), ("parent", D), ("open", D)],
        S::Unsupported => {
            &[("rect", D), ("actions", P), ("rawSubtypeCode", E), ("rawSubtypeName", E)]
        }
    }
}

/// Every field `subtype` declares, base first, in declaration order
/// (`ANNOTATION_FIELD_NAMES`), with its owner.
pub fn declared_fields(
    subtype: AnnotationSubtype,
) -> impl Iterator<Item = (&'static str, FieldOwner)> {
    let (before_rect, after_rect) = BASE_FIELDS.split_at(5);
    let own = kind_fields(subtype);
    before_rect
        .iter()
        .copied()
        .chain(own.iter().copied().filter(|(name, _)| *name == "rect"))
        .chain(after_rect.iter().copied())
        .chain(own.iter().copied().filter(|(name, _)| *name != "rect"))
}

pub(crate) fn owner_of(subtype: AnnotationSubtype, name: &str) -> Option<FieldOwner> {
    declared_fields(subtype).find(|(n, _)| *n == name).map(|(_, owner)| owner)
}

/// The resources a kind takes (`ANNOTATION_RESOURCE_ROLES`): a stamp
/// requires its `appearance`, a file attachment its `file`, and no other
/// kind takes either.
fn resource_roles(subtype: AnnotationSubtype) -> (bool, bool) {
    (subtype == AnnotationSubtype::Stamp, subtype == AnnotationSubtype::FileAttachment)
}

/// The bytes beside a draft, by role (`WireAnnotationResources`).
#[derive(Debug, Clone, Copy, Default)]
pub struct DraftResources<'a> {
    pub appearance: Option<&'a [u8]>,
    pub file: Option<&'a [u8]>,
}

/// An `#rrggbb` colour, read into its components (`ColorSchema`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub const BLACK: Self = Self(0, 0, 0);
    pub const RED: Self = Self(255, 0, 0);
    pub const YELLOW: Self = Self(255, 255, 0);

    /// `#rrggbb`, read into its components.
    pub fn parse_hex(text: &str) -> Option<Self> {
        Self::parse(text)
    }

    fn parse(text: &str) -> Option<Self> {
        let hex = text
            .strip_prefix('#')
            .filter(|hex| hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit()))?;
        let byte = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).ok();
        Some(Self(byte(0)?, byte(2)?, byte(4)?))
    }

    /// `#rrggbb`, lowercase, as the engine writes a colour.
    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }
}

/// `reply`: what the new annotation replies to, and how; a write may leave
/// out the type (`AnnotationReplyWriteSchema`).
#[derive(Debug, Clone, PartialEq)]
pub struct ReplyDraft {
    pub to: AnnotationRef,
    pub reply_type: AnnotationReplyType,
}

/// What a link may point at on a write (`PdfLinkTargetWritable`).
#[derive(Debug, Clone, PartialEq)]
pub enum LinkTargetWrite {
    Goto(PdfDestination),
    Uri(String),
    /// One of the standard page verbs.
    Named(String),
}

/// A file attachment's file, without its bytes (`AttachmentFileWriteSchema`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AttachmentFileWrite {
    pub name: String,
    pub mime_type: Option<String>,
    pub description: Option<String>,
}

/// A stamp's fit (`StampFitSchema`), `None` on the draft's `fit: null`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StampFit {
    Contain,
    Cover,
    Fill,
}

impl StampFit {
    pub fn native(self) -> pdfium::StampFit {
        match self {
            Self::Contain => pdfium::StampFit::Contain,
            Self::Cover => pdfium::StampFit::Cover,
            Self::Fill => pdfium::StampFit::Stretch,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Contain => "contain",
            Self::Cover => "cover",
            Self::Fill => "fill",
        }
    }
}

/// A field that may be absent, `null` or a value: `None` absent,
/// `Some(None)` null.
pub type Nullable<T> = Option<Option<T>>;

/// Every data field a create may carry, across the kinds, each read into
/// its type. Which ones a kind may carry is [`kind_fields`]; a field another
/// kind declares never gets here.
#[derive(Debug, Clone, Default)]
pub struct DraftFields {
    // The base.
    pub nm: Nullable<String>,
    pub rect: Option<PdfRect>,
    pub contents: Nullable<String>,
    pub subject: Nullable<String>,
    pub blend_mode: Option<BlendMode>,
    /// The `/F` flags the draft names, in [`FLAG_NAMES`] order.
    pub flags: [Option<bool>; 10],
    pub reply: Nullable<ReplyDraft>,
    pub group_id: Nullable<String>,
    // Style.
    pub color: Option<Rgb>,
    pub opacity: Option<f64>,
    pub stroke_width: Option<f64>,
    pub border_style: Option<AnnotationBorderStyle>,
    pub dash_array: Nullable<Vec<f64>>,
    pub interior_color: Nullable<Rgb>,
    // Geometry.
    pub quad_points: Option<Vec<NamedPdfQuad>>,
    pub r#box: Option<PdfRect>,
    pub rotation: Nullable<f64>,
    pub cloudy_intensity: Nullable<f64>,
    pub vertices: Option<Vec<PdfPoint>>,
    pub line_points: Option<LinePoints>,
    pub line_endings: Option<LineEndings>,
    pub ink_list: Option<InkList>,
    /// `/IT`, as the wire spells it; each kind reads its own vocabulary.
    pub intent: Nullable<String>,
    // A note.
    pub note_icon: Option<NoteIcon>,
    pub open: Option<bool>,
    pub state: Nullable<String>,
    pub state_model: Nullable<String>,
    // Text.
    pub font_family: Option<String>,
    pub font_size: Option<f64>,
    /// A free text's alignment includes `justify` (`RichTextAlignSchema`).
    pub text_align: Option<String>,
    pub vertical_align: Option<String>,
    pub rich_text: Option<RichTextDocumentInput>,
    pub font_color: Option<Rgb>,
    pub callout_line: Nullable<CalloutLine>,
    pub line_ending: Nullable<LineEnding>,
    // A stamp.
    pub name: Nullable<String>,
    pub fit: Nullable<StampFit>,
    // A redaction.
    pub overlay_text: Nullable<String>,
    pub repeat: Option<bool>,
    // A link.
    pub target: Nullable<LinkTargetWrite>,
    // A file attachment.
    pub file_icon: Option<FileAttachmentIcon>,
    pub file: Nullable<AttachmentFileWrite>,
    // A popup.
    pub open_popup: Option<bool>,
    // A measurement's caption, as far as it is written before 8.25.
    /// Whether the draft names any caption field (`touchesCaption`).
    pub caption_touched: bool,
    pub caption_enabled: Nullable<bool>,
    // A widget.
    /// Its style fields, nullable where the widget kind's are.
    pub widget: super::widget::WidgetStyle,
    /// Its turn, a quarter turn clockwise; `null` and 0 upright.
    pub widget_rotation: Nullable<crate::geom::PdfRotation>,
    /// Its actions, by event; `null` removes them all.
    pub widget_actions: Nullable<crate::features::actions::write::WidgetActionsPatch>,
}

/// The `/F` flag fields, in `ANNOT_FLAG_BITS` order (bit 0 first).
pub const FLAG_NAMES: [&str; 10] = [
    "invisible",
    "hidden",
    "print",
    "noZoom",
    "noRotate",
    "noView",
    "readOnly",
    "locked",
    "toggleNoView",
    "lockedContents",
];

/// A create, checked and stated whole, in the file's coordinates: what the
/// writers write (`PlacedDraft`). A box kind states its `box`.
#[derive(Debug, Clone)]
pub struct PreparedDraft {
    pub subtype: AnnotationSubtype,
    pub fields: DraftFields,
}

impl PreparedDraft {
    /// The `nm` the create writes, when it names a usable one: an empty
    /// name is no name, and the create stamps a fresh one.
    pub fn nm(&self) -> Option<&str> {
        self.fields.nm.as_ref().and_then(Option::as_deref).filter(|nm| !nm.is_empty())
    }
}

/// A refusal naming `field`. `kind` is the kind and the write, `"square
/// create"` or `"square update"`, as `invalidWrite` names it.
fn invalid(kind: &str, field: &str, message: impl std::fmt::Display) -> EngineError {
    EngineError::with_message(
        EngineErrorCode::InvalidArg,
        format!("{kind} field '{field}': {message}"),
    )
    .with_details(serde_json::json!({ "field": field }))
}

fn read<T: DeserializeOwned>(subtype: &str, name: &str, value: &Value) -> Result<T, EngineError> {
    serde_json::from_value(value.clone()).map_err(|e| invalid(subtype, name, e))
}

fn read_nullable<T: DeserializeOwned>(
    subtype: &str,
    name: &str,
    value: &Value,
) -> Result<Option<T>, EngineError> {
    if value.is_null() {
        return Ok(None);
    }
    read(subtype, name, value).map(Some)
}

fn finite(subtype: &str, name: &str, value: f64) -> Result<f64, EngineError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(invalid(subtype, name, "expected a finite number"))
    }
}

fn read_rect(subtype: &str, name: &str, value: &Value) -> Result<PdfRect, EngineError> {
    let rect: PdfRect = read(subtype, name, value)?;
    for edge in [rect.left, rect.bottom, rect.right, rect.top] {
        finite(subtype, name, edge)?;
    }
    Ok(rect)
}

fn read_color(subtype: &str, name: &str, value: &Value) -> Result<Rgb, EngineError> {
    let text: String = read(subtype, name, value)?;
    Rgb::parse(&text).ok_or_else(|| invalid(subtype, name, "expected a color as '#rrggbb'"))
}

fn read_number(
    subtype: &str,
    name: &str,
    value: &Value,
    check: impl Fn(f64) -> bool,
    what: &str,
) -> Result<f64, EngineError> {
    let number: f64 = read(subtype, name, value)?;
    if check(number) { Ok(number) } else { Err(invalid(subtype, name, what)) }
}

fn non_empty(subtype: &str, name: &str, value: &Value) -> Result<String, EngineError> {
    let text: String = read(subtype, name, value)?;
    if text.is_empty() {
        return Err(invalid(subtype, name, "expected at least one character"));
    }
    Ok(text)
}

fn one_of(
    subtype: &str,
    name: &str,
    value: &Value,
    allowed: &[&str],
) -> Result<String, EngineError> {
    let text: String = read(subtype, name, value)?;
    if allowed.contains(&text.as_str()) {
        Ok(text)
    } else {
        Err(invalid(subtype, name, format!("expected one of {}", allowed.join(", "))))
    }
}

/// The intents a kind writes (`*IntentSchema`, `FreeTextIntentWriteSchema`).
fn intents_of(subtype: AnnotationSubtype) -> &'static [&'static str] {
    use AnnotationSubtype as S;
    match subtype {
        S::Strikeout => &["strikeout-text-edit"],
        S::Ink => &["ink-highlight"],
        S::Caret => &["replace"],
        S::FreeText => &["free-text", "free-text-callout"],
        S::Line => &["line-dimension", "line-arrow"],
        S::Polygon => &["polygon-dimension", "polygon-cloud"],
        S::Polyline => &["polyline-dimension"],
        _ => &[],
    }
}

/// Read one data field of `subtype` into `fields`, as `verb` (`create` or
/// `update`) takes it.
pub(crate) fn read_field(
    verb: &str,
    subtype: AnnotationSubtype,
    name: &str,
    value: &Value,
    fields: &mut DraftFields,
) -> Result<(), EngineError> {
    use AnnotationSubtype as S;
    let kind = format!("{} {verb}", subtype.as_str());
    let kind = kind.as_str();
    let positive = |n: f64| n.is_finite() && n > 0.0;
    let nonnegative = |n: f64| n.is_finite() && n >= 0.0;
    if let Some(bit) = FLAG_NAMES.iter().position(|flag| *flag == name) {
        fields.flags[bit] = Some(read(kind, name, value)?);
        return Ok(());
    }
    // A widget's own fields (`WidgetDeclaration`): its style, nullable where
    // a shape's isn't, its quarter turn and its actions.
    if subtype == S::Widget {
        if super::widget::read_style_field(kind, name, value, &mut fields.widget)? {
            return Ok(());
        }
        match name {
            "rotation" => {
                let degrees: Option<f64> = read_nullable(kind, name, value)?;
                let turn = match degrees {
                    None => None,
                    Some(degrees) => super::widget::quarter_turn(degrees)
                        .filter(|_| [0.0, 90.0, 180.0, 270.0].contains(&degrees))
                        .ok_or_else(|| {
                            invalid(kind, name, "expected a quarter turn: 0, 90, 180 or 270")
                        })?,
                };
                fields.widget_rotation = Some(turn);
                return Ok(());
            }
            "actions" => {
                fields.widget_actions = Some(read_nullable(kind, name, value)?);
                return Ok(());
            }
            _ => {}
        }
    }
    match name {
        "nm" => fields.nm = Some(read_nullable(kind, name, value)?),
        "rect" => fields.rect = Some(read_rect(kind, name, value)?),
        "contents" => fields.contents = Some(read_nullable(kind, name, value)?),
        "subject" => fields.subject = Some(read_nullable(kind, name, value)?),
        "blendMode" => fields.blend_mode = Some(read(kind, name, value)?),
        "reply" => {
            fields.reply = Some(if value.is_null() {
                None
            } else {
                #[derive(serde::Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Reply {
                    to: AnnotationRef,
                    #[serde(default, rename = "type")]
                    reply_type: Option<String>,
                }
                let reply: Reply = read(kind, name, value)?;
                let reply_type = match reply.reply_type.as_deref() {
                    None | Some("reply") => AnnotationReplyType::Reply,
                    Some("group") => AnnotationReplyType::Group,
                    Some(other) => {
                        return Err(invalid(kind, name, format!("unknown reply type '{other}'")));
                    }
                };
                Some(ReplyDraft { to: reply.to, reply_type })
            });
        }
        "groupId" => {
            fields.group_id =
                Some(if value.is_null() { None } else { Some(non_empty(kind, name, value)?) });
        }
        "color" => fields.color = Some(read_color(kind, name, value)?),
        "fontColor" => fields.font_color = Some(read_color(kind, name, value)?),
        "interiorColor" => {
            fields.interior_color =
                Some(if value.is_null() { None } else { Some(read_color(kind, name, value)?) });
        }
        "opacity" => {
            fields.opacity = Some(read_number(
                kind,
                name,
                value,
                |n| (0.0..=1.0).contains(&n),
                "expected a number from 0 to 1",
            )?);
        }
        "strokeWidth" => {
            // Ink has no fill, so a line of 0 draws nothing.
            fields.stroke_width = Some(if subtype == S::Ink {
                read_number(kind, name, value, positive, "expected a number above 0")?
            } else {
                read_number(kind, name, value, nonnegative, "expected a number of 0 or more")?
            });
        }
        "borderStyle" => {
            let style: AnnotationBorderStyle = read(kind, name, value)?;
            // `beveled` and `inset` are read and kept; only widgets draw them.
            if !matches!(style, AnnotationBorderStyle::Solid | AnnotationBorderStyle::Dashed) {
                return Err(invalid(kind, name, "expected one of solid, dashed"));
            }
            fields.border_style = Some(style);
        }
        "dashArray" => {
            let dashes: Option<Vec<f64>> = read_nullable(kind, name, value)?;
            if dashes.iter().flatten().any(|d| !nonnegative(*d)) {
                return Err(invalid(kind, name, "expected numbers of 0 or more"));
            }
            fields.dash_array = Some(dashes);
        }
        "quadPoints" => {
            let quads: Vec<NamedPdfQuad> = read(kind, name, value)?;
            // A text markup needs a quad; a redaction may mark an area.
            if quads.is_empty() && subtype != S::Redact {
                return Err(invalid(kind, name, "expected at least one quad"));
            }
            fields.quad_points = Some(quads);
        }
        "box" => fields.r#box = Some(read_rect(kind, name, value)?),
        "rotation" => {
            let rotation: Option<f64> = read_nullable(kind, name, value)?;
            if let Some(degrees) = rotation {
                finite(kind, name, degrees)?;
            }
            fields.rotation = Some(rotation);
        }
        "cloudyIntensity" => {
            fields.cloudy_intensity = Some(if value.is_null() {
                None
            } else {
                Some(read_number(kind, name, value, positive, "expected a number above 0")?)
            });
        }
        "vertices" => {
            let vertices: Vec<PdfPoint> = read(kind, name, value)?;
            // A polygon of fewer than three points encloses nothing; a
            // polyline of fewer than two draws nothing.
            let least = if subtype == S::Polygon { 3 } else { 2 };
            if vertices.len() < least {
                return Err(invalid(kind, name, format!("expected at least {least} points")));
            }
            fields.vertices = Some(vertices);
        }
        "linePoints" => fields.line_points = Some(read(kind, name, value)?),
        "lineEndings" => fields.line_endings = Some(read(kind, name, value)?),
        "inkList" => {
            let ink: InkList = read(kind, name, value)?;
            if ink.is_empty() || ink.iter().any(Vec::is_empty) {
                return Err(invalid(
                    kind,
                    name,
                    "expected at least one stroke, and no stroke without points",
                ));
            }
            fields.ink_list = Some(ink);
        }
        "intent" => {
            fields.intent = Some(if value.is_null() {
                None
            } else {
                Some(one_of(kind, name, value, intents_of(subtype))?)
            });
        }
        "icon" if subtype == S::Text => fields.note_icon = Some(read(kind, name, value)?),
        "icon" => fields.file_icon = Some(read(kind, name, value)?),
        "open" if subtype == S::Popup => fields.open_popup = Some(read(kind, name, value)?),
        "open" => fields.open = Some(read(kind, name, value)?),
        "state" | "stateModel" => {
            let text = if value.is_null() { None } else { Some(non_empty(kind, name, value)?) };
            if name == "state" {
                fields.state = Some(text);
            } else {
                fields.state_model = Some(text);
            }
        }
        "fontFamily" => fields.font_family = Some(non_empty(kind, name, value)?),
        "fontSize" => {
            // A redaction's 0 fits the label to its region.
            fields.font_size = Some(if subtype == S::FreeText {
                read_number(kind, name, value, positive, "expected a number above 0")?
            } else {
                read_number(kind, name, value, nonnegative, "expected a number of 0 or more")?
            });
        }
        "textAlign" => {
            let allowed: &[&str] = if subtype == S::FreeText {
                &["left", "center", "right", "justify"]
            } else {
                &["left", "center", "right"]
            };
            fields.text_align = Some(one_of(kind, name, value, allowed)?);
        }
        "verticalAlign" => {
            fields.vertical_align = Some(one_of(kind, name, value, &["top", "middle", "bottom"])?);
        }
        "richText" => fields.rich_text = Some(read(kind, name, value)?),
        "calloutLine" => fields.callout_line = Some(read_nullable(kind, name, value)?),
        "lineEnding" => fields.line_ending = Some(read_nullable(kind, name, value)?),
        "name" => fields.name = Some(read_nullable(kind, name, value)?),
        "fit" => {
            fields.fit = Some(if value.is_null() {
                None
            } else {
                Some(match one_of(kind, name, value, &["contain", "cover", "fill"])?.as_str() {
                    "contain" => StampFit::Contain,
                    "cover" => StampFit::Cover,
                    _ => StampFit::Fill,
                })
            });
        }
        "overlayText" => fields.overlay_text = Some(read_nullable(kind, name, value)?),
        "repeat" => fields.repeat = Some(read(kind, name, value)?),
        "target" => fields.target = Some(read_target(kind, name, value)?),
        "file" => {
            fields.file = Some(if value.is_null() {
                None
            } else {
                // `AttachmentFileWriteSchema` strips the keys it doesn't
                // name: what a read adds (size, checksum, dates) is ignored,
                // so a read can be sent back.
                #[derive(serde::Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct File {
                    name: String,
                    #[serde(default)]
                    mime_type: Option<String>,
                    #[serde(default)]
                    description: Option<String>,
                }
                let file: File = read(kind, name, value)?;
                if file.name.is_empty() {
                    return Err(invalid(kind, "file.name", "expected at least one character"));
                }
                Some(AttachmentFileWrite {
                    name: file.name,
                    mime_type: file.mime_type,
                    description: file.description,
                })
            });
        }
        // A measurement's scale, caption and leader (`measureField`,
        // `shapeCaptionFields`, the line's caption and leader).
        // re-port (Task 8): 8.25 writes the scale, a shown or placed
        // caption and a leader, with the label they derive. Until then only
        // the values a read of an unmeasured shape gives can be written: no
        // scale, no leader, no offset or centre, a hidden caption, inline.
        "measure" | "leader" | "captionOffset" | "captionCenter" => {
            if !value.is_null() {
                return Err(measurement_not_ported(kind, name));
            }
            if name != "measure" && name != "leader" {
                fields.caption_touched = true;
            }
        }
        "captionEnabled" => {
            let enabled: Option<bool> = read_nullable(kind, name, value)?;
            if enabled == Some(true) {
                return Err(measurement_not_ported(kind, name));
            }
            fields.caption_touched = true;
            fields.caption_enabled = Some(enabled);
        }
        "captionPosition" => {
            if one_of(kind, name, value, &["inline", "top"])? != "inline" {
                return Err(measurement_not_ported(kind, name));
            }
            fields.caption_touched = true;
        }
        // A widget is the form's: refused before its fields are read.
        _ => return Err(invalid(kind, name, "not a field this engine writes")),
    }
    Ok(())
}

/// A measurement field whose value needs 8.25's writing.
fn measurement_not_ported(kind: &str, name: &str) -> EngineError {
    EngineError::with_message(
        EngineErrorCode::NotImplemented,
        format!("{kind} field '{name}': measurements are written by Task 8.25"),
    )
    .with_details(serde_json::json!({ "field": name }))
}

fn read_target(
    kind: &str,
    name: &str,
    value: &Value,
) -> Result<Option<LinkTargetWrite>, EngineError> {
    if value.is_null() {
        return Ok(None);
    }
    #[derive(serde::Deserialize)]
    #[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
    enum Target {
        Goto { destination: PdfDestination },
        Uri { uri: String },
        Named { name: String },
    }
    Ok(Some(match read(kind, name, value)? {
        Target::Goto { destination } => LinkTargetWrite::Goto(destination),
        Target::Uri { uri } => LinkTargetWrite::Uri(uri),
        Target::Named { name: verb } => {
            if !["NextPage", "PrevPage", "FirstPage", "LastPage"].contains(&verb.as_str()) {
                return Err(invalid(
                    kind,
                    name,
                    "expected a named verb of NextPage, PrevPage, FirstPage, LastPage",
                ));
            }
            LinkTargetWrite::Named(verb)
        }
    }))
}

/// The data fields a kind requires on a create.
fn required_fields(subtype: AnnotationSubtype) -> &'static [&'static str] {
    use AnnotationSubtype as S;
    match subtype {
        S::Highlight | S::Underline | S::Squiggly | S::Strikeout => &["quadPoints"],
        S::Polygon | S::Polyline => &["vertices"],
        S::Line => &["linePoints"],
        S::Ink => &["inkList"],
        S::Link => &["rect", "target"],
        S::Text | S::FileAttachment => &["rect"],
        S::Popup => &["rect", "parent"],
        _ => &[],
    }
}

/// The subtype a draft names.
fn subtype_of(draft: &Map<String, Value>) -> Result<AnnotationSubtype, EngineError> {
    let named = draft.get("subtype").and_then(Value::as_str).ok_or_else(|| {
        EngineError::with_message(EngineErrorCode::InvalidArg, "a create names its subtype")
            .with_details(serde_json::json!({ "field": "subtype" }))
    })?;
    serde_json::from_value(Value::String(named.to_owned())).map_err(|_| {
        EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!("no annotation kind '{named}'"),
        )
        .with_details(serde_json::json!({ "field": "subtype" }))
    })
}

/// Whether `subtype` is drawn about a box that can turn (its `spaces` give
/// `box`): square, circle, free text, stamp, caret and widget.
pub fn is_box_kind(subtype: AnnotationSubtype) -> bool {
    use AnnotationSubtype as S;
    matches!(subtype, S::Square | S::Circle | S::FreeText | S::Stamp | S::Caret | S::Widget)
}

/// `assertDeclaredFields`: a write names only fields its kind declares; a
/// misspelled or foreign field is refused, never ignored.
fn assert_declared_fields(
    subtype: AnnotationSubtype,
    draft: &Map<String, Value>,
) -> Result<(), EngineError> {
    let unknown: Vec<String> = draft
        .keys()
        .filter(|name| *name != "subtype" && owner_of(subtype, name).is_none())
        .map(|name| format!("'{name}'"))
        .collect();
    if unknown.is_empty() {
        return Ok(());
    }
    Err(EngineError::with_message(
        EngineErrorCode::InvalidArg,
        format!("{} has no field {}", subtype.as_str(), unknown.join(", ")),
    ))
}

/// `boxDraftFollows`: a box kind's create placed by where it stands. A
/// `rect` at a quarter turn pins the box down (`pdfQuarterTurnBox`); a
/// create that gives `box` keeps it. Refused: neither, a `rect` that isn't
/// finite, and a `rect` at another angle.
fn box_draft_follows(
    subtype: AnnotationSubtype,
    draft: &mut Map<String, Value>,
) -> Result<(), EngineError> {
    if !is_box_kind(subtype) || draft.contains_key("box") {
        return Ok(());
    }
    let kind = subtype.as_str();
    let Some(rect) = draft.get("rect") else {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!("{kind} create: give its `box`, or its `rect` with a quarter turn"),
        )
        .with_details(serde_json::json!({ "field": "box" })));
    };
    let create = format!("{kind} create");
    let rect = read_rect(&create, "rect", rect)
        .map_err(|_| invalid(&create, "rect", "not a rect of finite numbers"))?;
    let rotation = draft.get("rotation").and_then(Value::as_f64).unwrap_or(0.0);
    let turn = quarter_turn_of(rotation).ok_or_else(|| {
        invalid(
            &create,
            "rect",
            format!("a rect places a box at a quarter turn; at {rotation}° give its `box`"),
        )
    })?;
    draft.remove("rect");
    let placed = crate::features::annotation::read::record::pdf_quarter_turn_box(
        crate::geom::normalize_pdf_rect(rect),
        turn,
    );
    draft.insert("box".into(), serde_json::to_value(placed).map_err(|e| invalid(kind, "box", e))?);
    Ok(())
}

/// `quarterTurnOf`: a turn of 0, 90, 180 or 270 degrees (a negative turn,
/// or one past a whole turn, is the same turn), as the rotation it is;
/// `None` for any other angle. `Some(None)` upright.
fn quarter_turn_of(degrees: f64) -> Option<Option<crate::geom::PdfRotation>> {
    use crate::geom::PdfRotation as R;
    let turn = degrees.rem_euclid(360.0);
    if turn % 90.0 != 0.0 {
        return None;
    }
    Some(match turn as u32 {
        90 => Some(R::Clockwise90),
        180 => Some(R::Clockwise180),
        270 => Some(R::Clockwise270),
        _ => None,
    })
}

/// `pdfPrepareAnnotationDraft` then `assertAnnotationDraft`: the draft's
/// names checked, a box kind placed, then each field checked against its
/// kind. `linked` fields (a change set's `reply`, a popup's `parent`) are
/// taken out by the caller and linked by it.
pub fn check_draft(mut draft: Map<String, Value>) -> Result<PreparedDraft, EngineError> {
    let subtype = subtype_of(&draft)?;
    let kind = subtype.as_str();
    assert_declared_fields(subtype, &draft)?;
    box_draft_follows(subtype, &mut draft)?;
    for required in required_fields(subtype) {
        // The popup's parent is linked by the caller, never written here.
        if *required == "parent" {
            continue;
        }
        if !draft.contains_key(*required) {
            return Err(invalid(&format!("{kind} create"), required, "Required"));
        }
    }
    let mut fields = DraftFields::default();
    for (name, value) in &draft {
        if name == "subtype" || name == "parent" {
            continue;
        }
        match owner_of(subtype, name) {
            Some(FieldOwner::Data) => read_field("create", subtype, name, value, &mut fields)?,
            // An engine field the engine works out (`readBack`), sent back
            // as a read has it: checked as a read, then the engine's own.
            Some(FieldOwner::Engine) if name == "rect" => {
                read_rect(&format!("{kind} create"), name, value)?;
            }
            // Accepted so a read can be sent back; nothing writes them.
            _ => {}
        }
    }
    Ok(PreparedDraft { subtype, fields })
}

/// `standardStateModelOf`: the model a standard state belongs to.
pub(crate) fn standard_state_model_of(state: &str) -> Option<&'static str> {
    match state {
        "accepted" | "rejected" | "cancelled" | "completed" | "none" => Some("review"),
        "marked" | "unmarked" => Some("marked"),
        _ => None,
    }
}

/// What a font name asks of a rich body (`bodyWithStyle`'s face half).
fn body_with_style(
    mut body: Map<String, Value>,
    fields: &DraftFields,
    context: &AnnotationWriteContext<'_>,
) -> Map<String, Value> {
    if let Some(family) = fields.font_family.as_deref() {
        let describe = |key: &str| context.describe_font(key);
        let face = face_for_free_text_font(family, Some(&describe));
        body.insert("family".into(), Value::String(face.family));
        if let Some(weight) = face.weight {
            body.insert("weight".into(), Value::from(weight));
        }
        if let Some(italic) = face.italic {
            body.insert("italic".into(), Value::Bool(italic));
        }
    }
    if let Some(size) = fields.font_size {
        body.insert("size".into(), Value::from(size));
    }
    if let Some(color) = fields.font_color {
        body.insert("color".into(), Value::String(color.hex()));
    }
    if let Some(align) = fields.text_align.as_deref() {
        body.insert("align".into(), Value::String(align.to_owned()));
    }
    body
}

/// `pdfDeriveAnnotationDraft`: everything that follows from a checked draft
/// stated. A note's standard state brings its model; a free text states its
/// intent and its text as rich text carrying its text style, `contents`
/// its plain projection; a redaction over text without a `rect` covers its
/// quads. Refused where nothing can follow: a custom state with no model,
/// `contents` that disagree with the rich text, a callout line without the
/// callout intent.
pub fn derive_draft(
    mut draft: PreparedDraft,
    context: &AnnotationWriteContext<'_>,
) -> Result<PreparedDraft, EngineError> {
    use AnnotationSubtype as S;
    let fields = &mut draft.fields;
    // `assertRichTextAgreement`.
    if draft.subtype == S::FreeText
        && let (Some(Some(contents)), Some(rich)) = (&fields.contents, &fields.rich_text)
        && *contents != plain_text(&rich.paragraphs)
    {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "free-text: `contents` must equal the plain projection of `richText` when both are given",
        ));
    }
    match draft.subtype {
        // `noteDraftStateFollows`.
        S::Text => {
            if let Some(Some(state)) = &fields.state
                && !matches!(fields.state_model, Some(Some(_)))
            {
                let model = standard_state_model_of(state).ok_or_else(|| {
                    EngineError::with_message(
                        EngineErrorCode::InvalidArg,
                        format!("text: the custom state '{state}' needs a stateModel"),
                    )
                    .with_details(serde_json::json!({ "field": "stateModel" }))
                })?;
                fields.state_model = Some(Some(model.to_owned()));
            }
        }
        // `freeTextDraftFollows`.
        S::FreeText => {
            let has_callout = matches!(fields.callout_line, Some(Some(_)));
            let intent = fields.intent.clone().flatten().unwrap_or_else(|| {
                if has_callout { "free-text-callout" } else { "free-text" }.to_owned()
            });
            if has_callout && intent != "free-text-callout" {
                return Err(EngineError::with_message(
                    EngineErrorCode::InvalidArg,
                    format!(
                        "free-text: a callout line needs intent 'free-text-callout', not '{intent}'"
                    ),
                )
                .with_details(serde_json::json!({ "field": "intent" })));
            }
            fields.intent = Some(Some(intent));
            let given = fields.rich_text.take();
            let base = given.as_ref().and_then(|rich| rich.body.clone()).unwrap_or_default();
            let paragraphs: Vec<RichTextParagraph> = match given {
                Some(rich) => rich.paragraphs,
                None => paragraphs_from_plain_text(
                    fields.contents.as_ref().and_then(Option::as_deref).unwrap_or(""),
                ),
            };
            let body = body_with_style(base, fields, context);
            let rich = RichTextDocumentInput { body: Some(body), paragraphs };
            // The engine writes `/Contents` from the rich text: a line break
            // reads back as `\r`.
            fields.contents = Some(Some(plain_text(&rich.paragraphs)));
            fields.rich_text = Some(rich);
        }
        // `redactDraftFollows`.
        S::Redact => {
            if fields.rect.is_none()
                && let Some(quads) = fields.quad_points.as_ref().filter(|q| !q.is_empty())
            {
                let corners: Vec<PdfPoint> = quads
                    .iter()
                    .flat_map(|q| [q.upper_left, q.upper_right, q.lower_left, q.lower_right])
                    .collect();
                fields.rect = Some(crate::geom::pdf_points_bounds(&corners));
            }
        }
        _ => {}
    }
    Ok(draft)
}

/// `assertAnnotationResources`: a role the kind doesn't take is refused, and
/// so is a create (`create`) without the role it requires.
pub fn assert_resources(
    subtype: AnnotationSubtype,
    resources: &DraftResources<'_>,
    create: bool,
) -> Result<(), EngineError> {
    let (takes_appearance, takes_file) = resource_roles(subtype);
    let kind = subtype.as_str();
    for (role, given, takes) in [
        ("appearance", resources.appearance.is_some(), takes_appearance),
        ("file", resources.file.is_some(), takes_file),
    ] {
        if given && !takes {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!("a {kind} annotation takes no '{role}' resource"),
            ));
        }
        if !given && takes && create {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!("creating a {kind} annotation needs its '{role}' resource"),
            ));
        }
    }
    Ok(())
}

/// `preflightDraft`: each kind's own checks of what the schema can't see,
/// before the first write.
pub fn preflight_draft(
    draft: &PreparedDraft,
    resources: &DraftResources<'_>,
    context: &AnnotationWriteContext<'_>,
    document: &pdfium::Document,
) -> Result<(), EngineError> {
    use AnnotationSubtype as S;
    let fields = &draft.fields;
    match draft.subtype {
        // `preflightStampDraft`.
        S::Stamp => {
            if let Some(Some(name)) = &fields.name
                && name.is_empty()
            {
                return Err(EngineError::with_message(
                    EngineErrorCode::InvalidArg,
                    "stamp name must be non-empty",
                ));
            }
            super::stamp::require_stamp_content(document, resources.appearance)?;
        }
        // `preflightFileAttachmentDraft`.
        S::FileAttachment => {
            if !matches!(&fields.file, Some(Some(file)) if !file.name.is_empty()) {
                return Err(EngineError::with_message(
                    EngineErrorCode::InvalidArg,
                    "a file attachment needs the file's name: give file.name, or the bytes as a File",
                )
                .with_details(serde_json::json!({ "field": "file.name" })));
            }
        }
        // `preflightRedactDraft`.
        S::Redact
            if fields.rect.is_none() && fields.quad_points.as_ref().is_none_or(Vec::is_empty) =>
        {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                "redact create needs a rect or at least one of quadPoints",
            )
            .with_details(serde_json::json!({ "field": "rect" })));
        }
        _ => {}
    }
    // A font that isn't one of the fourteen is a key this thread must have
    // registered: found out now rather than after the annotation exists.
    if let Some(family) = fields.font_family.as_deref()
        && crate::features::annotation::vocabulary::StandardFont::from_family(family).is_none()
        && matches!(draft.subtype, S::FreeText | S::Redact)
    {
        context.resolve_font(family)?;
    }
    Ok(())
}

/// `prepareCreate`: everything a create checks before its first write, and
/// the draft as it is written. `draft` is in the file's coordinates, with a
/// change set's `reply` and a popup's `parent` still in it; they are checked
/// with the rest and linked by the caller.
pub fn prepare_create(
    draft: Map<String, Value>,
    resources: &DraftResources<'_>,
    context: &AnnotationWriteContext<'_>,
    document: &pdfium::Document,
) -> Result<PreparedDraft, EngineError> {
    let checked = check_draft(draft)?;
    let resolved = derive_draft(checked, context)?;
    assert_resources(resolved.subtype, resources, true)?;
    preflight_draft(&resolved, resources, context, document)?;
    Ok(resolved)
}

/// The text alignment a free text's or redaction's `/Q` takes; `justify`
/// has none (`textAlignmentToCode` maps it to left).
pub fn text_alignment_of(align: &str) -> TextAlignment {
    match align {
        "center" => TextAlignment::Center,
        "right" => TextAlignment::Right,
        _ => TextAlignment::Left,
    }
}

/// `/IT` names, from the wire's intents (`*IntentToName`).
pub fn intent_name(intent: &str) -> Option<&'static str> {
    Some(match intent {
        "strikeout-text-edit" => StrikeoutIntent::StrikeoutTextEdit.name(),
        "ink-highlight" => InkIntent::InkHighlight.name(),
        "replace" => CaretIntent::Replace.name(),
        "free-text" => FreeTextIntent::FreeText.name(),
        "free-text-callout" => FreeTextIntent::FreeTextCallout.name(),
        "free-text-typewriter" => FreeTextIntent::FreeTextTypewriter.name(),
        "line-dimension" => "LineDimension",
        "line-arrow" => "LineArrow",
        "polygon-dimension" => "PolygonDimension",
        "polygon-cloud" => "PolygonCloud",
        "polyline-dimension" => "PolyLineDimension",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn checked(draft: Value) -> Result<PreparedDraft, EngineError> {
        check_draft(draft.as_object().unwrap().clone())
    }

    #[test]
    fn a_field_the_kind_does_not_declare_is_refused() {
        let refused = checked(json!({ "subtype": "square", "box": {
            "left": 0, "bottom": 0, "right": 10, "top": 10 }, "quadPoints": [] }))
        .unwrap_err();
        assert_eq!(refused.code, EngineErrorCode::InvalidArg);
        assert!(refused.message.contains("square has no field 'quadPoints'"), "{refused}");
    }

    #[test]
    fn a_value_its_kind_refuses_names_the_field() {
        let refused = checked(json!({ "subtype": "square", "box": {
            "left": 0, "bottom": 0, "right": 10, "top": 10 }, "color": "red" }))
        .unwrap_err();
        assert_eq!(refused.code, EngineErrorCode::InvalidArg);
        assert_eq!(refused.details.unwrap()["field"], "color");
    }

    #[test]
    fn a_box_kind_is_placed_by_its_rect_only_at_a_quarter_turn() {
        let placed = checked(json!({ "subtype": "square", "rotation": 90, "rect": {
            "left": 0, "bottom": 0, "right": 20, "top": 10 } }))
        .unwrap();
        let rect = placed.fields.r#box.unwrap();
        assert_eq!(rect.right - rect.left, 10.0, "the sides swap under a quarter turn");
        let refused = checked(json!({ "subtype": "square", "rotation": 30, "rect": {
            "left": 0, "bottom": 0, "right": 20, "top": 10 } }))
        .unwrap_err();
        assert_eq!(refused.details.unwrap()["field"], "rect");
        let neither = checked(json!({ "subtype": "square" })).unwrap_err();
        assert_eq!(neither.details.unwrap()["field"], "box");
    }

    #[test]
    fn engine_and_attribution_fields_sent_back_are_accepted_and_ignored() {
        let draft = checked(json!({ "subtype": "text",
            "rect": { "left": 0, "bottom": 0, "right": 20, "top": 20 },
            "ref": "anything", "author": 3, "createdAt": "x", "hasAppearance": true }))
        .unwrap();
        assert!(draft.fields.rect.is_some());
    }

    #[test]
    fn a_standard_state_brings_its_model_and_a_custom_one_needs_one() {
        let context = AnnotationWriteContext::empty();
        let rect = json!({ "left": 0, "bottom": 0, "right": 20, "top": 20 });
        let derived = derive_draft(
            checked(json!({ "subtype": "text", "rect": rect, "state": "accepted" })).unwrap(),
            &context,
        )
        .unwrap();
        assert_eq!(derived.fields.state_model, Some(Some("review".to_owned())));
        let refused = derive_draft(
            checked(json!({ "subtype": "text", "rect": rect, "state": "AwaitingCounsel" }))
                .unwrap(),
            &context,
        )
        .unwrap_err();
        assert_eq!(refused.details.unwrap()["field"], "stateModel");
    }

    #[test]
    fn a_free_text_states_its_intent_and_its_text_as_rich_text() {
        let derived = derive_draft(
            checked(json!({ "subtype": "free-text",
                "box": { "left": 0, "bottom": 0, "right": 100, "top": 50 },
                "fontFamily": "times-bold", "fontSize": 18, "contents": "a\nb" }))
            .unwrap(),
            &AnnotationWriteContext::empty(),
        )
        .unwrap();
        assert_eq!(derived.fields.intent, Some(Some("free-text".to_owned())));
        assert_eq!(derived.fields.contents, Some(Some("a\rb".to_owned())));
        let body = derived.fields.rich_text.unwrap().body.unwrap();
        assert_eq!(body["family"], "Times");
        assert_eq!(body["weight"], 700);
        assert_eq!(body["size"], 18.0);
    }

    #[test]
    fn a_callout_line_needs_the_callout_intent() {
        let refused = derive_draft(
            checked(json!({ "subtype": "free-text", "intent": "free-text",
                "box": { "left": 0, "bottom": 0, "right": 100, "top": 50 },
                "calloutLine": [{ "x": 0, "y": 0 }, { "x": 10, "y": 10 }] }))
            .unwrap(),
            &AnnotationWriteContext::empty(),
        )
        .unwrap_err();
        assert_eq!(refused.details.unwrap()["field"], "intent");
    }

    #[test]
    fn a_resource_the_kind_does_not_take_or_needs_is_refused() {
        let bytes: &[u8] = b"x";
        let stray = DraftResources { appearance: Some(bytes), file: None };
        assert!(assert_resources(AnnotationSubtype::Square, &stray, true).is_err());
        assert!(
            assert_resources(AnnotationSubtype::Stamp, &DraftResources::default(), true).is_err()
        );
        assert!(assert_resources(AnnotationSubtype::Stamp, &stray, true).is_ok());
        // An update takes the role or leaves it out.
        assert!(
            assert_resources(AnnotationSubtype::Stamp, &DraftResources::default(), false).is_ok()
        );
        assert!(assert_resources(AnnotationSubtype::Square, &stray, false).is_err());
    }

    #[test]
    fn colors_read_as_rrggbb() {
        assert_eq!(Rgb::parse("#0080Ff"), Some(Rgb(0, 128, 255)));
        assert_eq!(Rgb::parse("0080ff"), None);
        assert_eq!(Rgb::parse("#0080f"), None);
        assert_eq!(Rgb(0, 128, 255).hex(), "#0080ff");
    }
}
