//! `types.ts`: the core's vocabulary. Records hold the engine's annotation
//! as JSON ([`Annotation`]); everything the core works out from one is
//! typed. Each object type remembers its member order (`object.rs`).

use std::fmt;
use std::rc::Rc;

use indexmap::IndexMap;
use serde_json::Value;

pub use epdf_geometry::{Point, Quad, QuadRing, Rect};

use crate::js::Record;
use crate::json::{self, Object};
use crate::object::{Json, JsonError, Nullable};
use crate::{js_object, js_strings, js_union};

pub type Id = String;
pub type Cursor = String;
/// An annotation kind's name (`kinds/`): its subtype, a widget's field
/// family kind (`widget-text`…), or `free-text-callout`.
pub type KindName = String;
pub type LineEnding = String;
/// Engine fields by name, as a create or an update states them.
pub type FieldValues = Object;
/// The engine's annotation, in page space: the only data a record holds.
pub type Annotation = Rc<Object>;
/// A draft or patch, as the engine takes it.
pub type AnnotationDraft = Object;
pub type AnnotationPatch = Object;

js_object! {
    /// A page address (`{ kind: 'objectNumber', objectNumber }`).
    pub struct PageRef {
        "kind" => pub kind: String,
        "objectNumber" => pub object_number: f64,
    }
}

impl PageRef {
    pub fn new(object_number: f64) -> Self {
        PageRef { kind: "objectNumber".into(), object_number, order: Default::default() }
    }

    /// `page.objectNumber` of a page ref read off a record's annotation.
    pub fn number_of(value: Option<&Value>) -> Option<f64> {
        value.and_then(|v| v.get("objectNumber")).and_then(json::number)
    }

    pub fn of_value(value: &Value) -> Option<PageRef> {
        PageRef::read(value).ok()
    }
}

js_object! {
    /// A line's endings; either may be missing (a tool's partial defaults).
    pub struct LineEndings {
        "start" => pub start: Option<LineEnding>,
        "end" => pub end: Option<LineEnding>,
    }
}

impl LineEndings {
    pub fn new(start: &str, end: &str) -> Self {
        LineEndings { start: Some(start.into()), end: Some(end.into()), order: Default::default() }
    }
}

js_object! {
    /// Where a text-edit annotation anchors: the boundary glyph's cell and the reading direction.
    pub struct TextEndAnchor {
        "glyphQuad" => pub glyph_quad: Quad,
        "advance" => pub advance: f64,
    }
}

js_object! {
    /// The per-page view a screen-anchored projection takes (`ViewEnv`).
    pub struct ViewEnv {
        "zoom" => pub zoom: f64,
        "rotation" => pub rotation: f64,
    }
}

impl ViewEnv {
    pub fn new(zoom: f64, rotation: f64) -> Self {
        ViewEnv { zoom, rotation, order: Default::default() }
    }
}

// ── shapes ──────────────────────────────────────────────────────────────────

js_object! {
    /// The box family's shape: the engine's box, its turn, and whether it draws the ellipse.
    pub struct BoxShape {
        "box" => pub box_: Rect,
        "rotation" => pub rotation: f64,
        "ellipse" => pub ellipse: bool,
    }
}

js_object! {
    pub struct LinePoints {
        "start" => pub start: Point,
        "end" => pub end: Point,
    }
}

impl LinePoints {
    pub fn new(start: Point, end: Point) -> Self {
        LinePoints { start, end, order: Default::default() }
    }
}

js_object! {
    /// A line: its two ends upright, and their endings.
    pub struct LineShape {
        "linePoints" => pub line_points: LinePoints,
        "lineEndings" => pub line_endings: Option<LineEndings>,
        "rotation" => pub rotation: f64,
    }
}

js_object! {
    /// A polygon (`closed`) or polyline: its vertices upright.
    pub struct PolyShape {
        "vertices" => pub vertices: Vec<Point>,
        "closed" => pub closed: bool,
        "lineEndings" => pub line_endings: Option<LineEndings>,
        /// A measurement's caption center, upright with the vertices; `null` places it automatically.
        "captionCenter" => pub caption_center: Option<Nullable<Point>>,
        "rotation" => pub rotation: f64,
    }
}

js_object! {
    /// Ink: its strokes upright.
    pub struct InkShape {
        "inkList" => pub ink_list: Vec<Vec<Point>>,
        "rotation" => pub rotation: f64,
    }
}

js_object! {
    /// Text markup's or a text redaction's quads.
    pub struct QuadsShape {
        "quadPoints" => pub quad_points: Vec<Quad>,
    }
}

js_object! {
    /// A caret: its box and its turn.
    pub struct CaretShape {
        "box" => pub box_: Rect,
        "rotation" => pub rotation: f64,
    }
}

js_object! {
    /// A free text: its box, its turn, and a callout's line.
    pub struct TextBoxShape {
        "box" => pub box_: Rect,
        "rotation" => pub rotation: f64,
        "calloutLine" => pub callout_line: Nullable<Vec<Point>>,
        "lineEnding" => pub line_ending: Nullable<LineEnding>,
    }
}

js_union! {
    /// An annotation's shape: what hit-testing, gestures and drawing work on.
    pub enum Shape ("kind") {
        "box" => Box(BoxShape),
        "line" => Line(LineShape),
        "poly" => Poly(PolyShape),
        "ink" => Ink(InkShape),
        "quads" => Quads(QuadsShape),
        "caret" => Caret(CaretShape),
        "text-box" => TextBox(TextBoxShape),
    }
}

impl Shape {
    pub fn kind(&self) -> &'static str {
        self.tag()
    }

    pub fn is_points(&self) -> bool {
        matches!(self, Shape::Line(_) | Shape::Poly(_) | Shape::Ink(_))
    }
}

// ── style ───────────────────────────────────────────────────────────────────

js_object! {
    /// How an annotation is drawn, by the engine's field names (`Style`).
    pub struct Style {
        "color" => pub color: Option<Value>,
        "interiorColor" => pub interior_color: Nullable<String>,
        "strokeWidth" => pub stroke_width: f64,
        "opacity" => pub opacity: Option<Value>,
        "blendMode" => pub blend_mode: Option<Value>,
        "borderStyle" => pub border_style: Option<Value>,
        "dashArray" => pub dash_array: Nullable<Vec<f64>>,
        "cloudyIntensity" => pub cloudy_intensity: Nullable<f64>,
    }
}

impl Style {
    /// The stroke colour as text (`null` or absent as `None`).
    pub fn color_str(&self) -> Option<&str> {
        self.color.as_ref().and_then(Value::as_str)
    }

    pub fn opacity_value(&self) -> Option<Value> {
        self.opacity.clone()
    }

    pub fn blend_str(&self) -> Option<&str> {
        self.blend_mode.as_ref().and_then(Value::as_str)
    }

    pub fn border_str(&self) -> Option<&str> {
        self.border_style.as_ref().and_then(Value::as_str)
    }

    /// The stroke a shape's bounds and drawing depend on.
    pub fn stroke(&self) -> Stroke {
        Stroke {
            stroke_width: self.stroke_width,
            cloudy_intensity: self.cloudy_intensity.as_option().copied(),
        }
    }
}

/// What a shape's bounds, hit test and drawing depend on besides the shape (`Stroke`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stroke {
    pub stroke_width: f64,
    pub cloudy_intensity: Option<f64>,
}

impl Stroke {
    pub fn width(stroke_width: f64) -> Self {
        Stroke { stroke_width, cloudy_intensity: None }
    }

    /// A cloud's intensity when it is truthy (`cloudyIntensity && …`).
    pub fn cloud(&self) -> Option<f64> {
        self.cloudy_intensity.filter(|i| *i != 0.0 && !i.is_nan())
    }

    /// Read `{ strokeWidth, cloudyIntensity? }` (a `Stroke` or a whole `Style`).
    pub fn of_value(value: &Value) -> Result<Self, JsonError> {
        let object = value.as_object().ok_or_else(|| JsonError::new("not a stroke"))?;
        Ok(Stroke {
            stroke_width: json::get_num(object, "strokeWidth").unwrap_or(f64::NAN),
            cloudy_intensity: json::get(object, "cloudyIntensity").and_then(json::number),
        })
    }
}

js_object! {
    /// How an annotation's text is set (`TextStyle`).
    pub struct TextStyle {
        "fontFamily" => pub font_family: Option<Value>,
        "fontSize" => pub font_size: Option<Value>,
        "fontColor" => pub font_color: Option<Value>,
        "textAlign" => pub text_align: Option<Value>,
        "bold" => pub bold: Option<bool>,
        "italic" => pub italic: Option<bool>,
        "underline" => pub underline: Option<bool>,
    }
}

impl TextStyle {
    pub fn size(&self) -> f64 {
        self.font_size.as_ref().and_then(json::number).unwrap_or(f64::NAN)
    }
    pub fn family(&self) -> Option<&str> {
        self.font_family.as_ref().and_then(Value::as_str)
    }
    pub fn color(&self) -> Option<&str> {
        self.font_color.as_ref().and_then(Value::as_str)
    }
    pub fn align(&self) -> Option<&str> {
        self.text_align.as_ref().and_then(Value::as_str)
    }
}

// ── records ─────────────────────────────────────────────────────────────────

js_strings! {
    /// How a record renders: the engine's raster or live.
    pub enum Source {
        "baked" => Baked,
        "vector" => Vector,
    }
}

js_object! {
    /// The session's authority over a record (permissions.md).
    pub struct Authority {
        "update" => pub update: bool,
        "delete" => pub delete: bool,
    }
}

/// A lent record's live drawing, from its lender (`ModelAnnotation.painter`).
#[derive(Clone, Default)]
pub struct Painter(pub Option<Rc<dyn Fn(Rect) -> Vec<SceneNode>>>);

impl Painter {
    pub fn is_some(&self) -> bool {
        self.0.is_some()
    }
}

impl fmt::Debug for Painter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.0.is_some() { "Painter(..)" } else { "Painter(None)" })
    }
}

impl PartialEq for Painter {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        }
    }
}

impl Json for Painter {
    fn from_json(_: Option<&Value>) -> Result<Self, JsonError> {
        Ok(Painter(None))
    }
    fn to_json(&self) -> Option<Value> {
        None
    }
}

js_object! {
    /// One annotation as the core works on it (`ModelAnnotation`).
    pub struct ModelAnnotation {
        "id" => pub id: Id,
        /// A record this session created that the engine hasn't written yet.
        "unconfirmed" => pub unconfirmed: Option<bool>,
        "source" => pub source: Source,
        "apBox" => pub ap_box: Option<Rect>,
        "apRot" => pub ap_rot: Option<f64>,
        "pictureVersion" => pub picture_version: Option<f64>,
        "painter" => pub painter: Painter,
        "authority" => pub authority: Option<Authority>,
        "annotation" => pub annotation: Annotation,
    }
}

impl ModelAnnotation {
    /// The record's subtype.
    pub fn subtype(&self) -> &str {
        json::get_str(&self.annotation, "subtype").unwrap_or("")
    }

    /// `record.annotation.page.objectNumber`.
    pub fn page_number(&self) -> Option<f64> {
        PageRef::number_of(self.annotation.get("page"))
    }

    pub fn page(&self) -> PageRef {
        self.annotation
            .get("page")
            .and_then(PageRef::of_value)
            .unwrap_or_else(|| PageRef::new(f64::NAN))
    }

    /// The record with `annotation`, the rest as it is (`{ ...record, annotation }`).
    pub fn with_annotation(&self, annotation: Object) -> ModelAnnotation {
        ModelAnnotation { annotation: Rc::new(annotation), ..self.clone() }
    }
}

// ── session ─────────────────────────────────────────────────────────────────

js_object! {
    /// An alignment guide from move snapping.
    pub struct Guide {
        "axis" => pub axis: String,
        "at" => pub at: f64,
        "lo" => pub lo: f64,
        "hi" => pub hi: f64,
    }
}

js_object! {
    /// Snapping settings (`SnapSettings`).
    pub struct SnapSettings {
        "alignment" => pub alignment: bool,
        "alignmentThreshold" => pub alignment_threshold: f64,
        "rotation" => pub rotation: bool,
        "rotationAngles" => pub rotation_angles: Vec<f64>,
        "rotationThreshold" => pub rotation_threshold: f64,
    }
}

/// A tool's click-create policy (`ClickCreate`): a box (`width`, `height`)
/// or a line (`length`, `rotation`), and its `anchor`. Kept as the object the
/// tool gave, members it doesn't know included, since a draft carries it.
#[derive(Clone, Debug, PartialEq)]
pub struct ClickCreate(pub Object);

impl ClickCreate {
    fn number(&self, name: &str) -> Option<f64> {
        json::get(&self.0, name).and_then(json::number)
    }
    pub fn width(&self) -> f64 {
        self.number("width").unwrap_or(f64::NAN)
    }
    pub fn height(&self) -> f64 {
        self.number("height").unwrap_or(f64::NAN)
    }
    /// `'length' in policy`: a line policy.
    pub fn is_line(&self) -> bool {
        self.0.contains_key("length")
    }
    pub fn length(&self) -> f64 {
        self.number("length").unwrap_or(f64::NAN)
    }
    pub fn rotation(&self) -> f64 {
        self.number("rotation").unwrap_or(0.0)
    }
    pub fn anchor(&self) -> Option<&str> {
        json::get_str(&self.0, "anchor")
    }
}

impl Json for ClickCreate {
    fn from_json(value: Option<&Value>) -> Result<Self, JsonError> {
        Object::from_json(value).map(ClickCreate)
    }
    fn to_json(&self) -> Option<Value> {
        Some(Value::Object(self.0.clone()))
    }
}

/// `ClickCreate | false`.
#[derive(Clone, Debug, PartialEq)]
pub enum ClickPolicy {
    Off,
    On(ClickCreate),
}

impl ClickPolicy {
    pub fn create(&self) -> Option<&ClickCreate> {
        match self {
            ClickPolicy::Off => None,
            ClickPolicy::On(c) => Some(c),
        }
    }
}

impl Json for ClickPolicy {
    fn from_json(value: Option<&Value>) -> Result<Self, JsonError> {
        match value {
            Some(Value::Bool(false)) => Ok(ClickPolicy::Off),
            other => ClickCreate::from_json(other).map(ClickPolicy::On),
        }
    }
    fn to_json(&self) -> Option<Value> {
        match self {
            ClickPolicy::Off => Some(Value::Bool(false)),
            ClickPolicy::On(c) => c.to_json(),
        }
    }
}

js_object! {
    pub struct CreateRect {
        "subtype" => pub subtype: KindName,
        "preset" => pub preset: Option<String>,
        "page" => pub page: PageRef,
        "from" => pub from: Point,
        "to" => pub to: Point,
        "displayRotation" => pub display_rotation: Option<f64>,
        "upright" => pub upright: Option<bool>,
        "clickCreate" => pub click_create: Option<ClickPolicy>,
        "flags" => pub flags: Option<Object>,
    }
}

js_object! {
    pub struct CreateDistance {
        "step" => pub step: String,
        "subtype" => pub subtype: KindName,
        "preset" => pub preset: String,
        "page" => pub page: PageRef,
        "from" => pub from: Point,
        "to" => pub to: Point,
        "measure" => pub measure: Object,
        "flags" => pub flags: Option<Object>,
    }
}

js_object! {
    pub struct CreateLine {
        "measure" => pub measure: Option<Object>,
        "capture" => pub capture: Option<String>,
        "subtype" => pub subtype: KindName,
        "preset" => pub preset: Option<String>,
        "page" => pub page: PageRef,
        "from" => pub from: Point,
        "to" => pub to: Point,
        "displayRotation" => pub display_rotation: Option<f64>,
        "upright" => pub upright: Option<bool>,
        "clickCreate" => pub click_create: Option<ClickPolicy>,
        "flags" => pub flags: Option<Object>,
    }
}

js_object! {
    pub struct CreatePoly {
        "subtype" => pub subtype: KindName,
        "preset" => pub preset: Option<String>,
        "page" => pub page: PageRef,
        "points" => pub points: Vec<Point>,
        "current" => pub current: Point,
        "closed" => pub closed: bool,
        "measure" => pub measure: Option<Object>,
        "flags" => pub flags: Option<Object>,
    }
}

js_object! {
    pub struct CreateInk {
        "subtype" => pub subtype: KindName,
        "preset" => pub preset: Option<String>,
        "page" => pub page: PageRef,
        "strokes" => pub strokes: Vec<Vec<Point>>,
        "intent" => pub intent: Option<String>,
        "flags" => pub flags: Option<Object>,
    }
}

js_object! {
    pub struct CreateCallout {
        "subtype" => pub subtype: KindName,
        "preset" => pub preset: Option<String>,
        "page" => pub page: PageRef,
        "step" => pub step: String,
        "tip" => pub tip: Point,
        "current" => pub current: Point,
        "displayRotation" => pub display_rotation: Option<f64>,
        "upright" => pub upright: Option<bool>,
        "pageBox" => pub page_box: Option<Rect>,
        "flags" => pub flags: Option<Object>,
        "knee" => pub knee: Option<Point>,
        "boxFrom" => pub box_from: Option<Point>,
        "boxTo" => pub box_to: Option<Point>,
    }
}

js_object! {
    pub struct MoveDraft {
        "ids" => pub ids: Vec<Id>,
        "start" => pub start: Point,
        "delta" => pub delta: Point,
        "guides" => pub guides: Vec<Guide>,
    }
}

js_object! {
    pub struct HandleDraft {
        "id" => pub id: Id,
        "handle" => pub handle: String,
        "base" => pub base: Shape,
        "current" => pub current: Shape,
        "view" => pub view: Option<ViewEnv>,
    }
}

js_object! {
    pub struct RotateDraft {
        "ids" => pub ids: Vec<Id>,
        "pivot" => pub pivot: Point,
        "start" => pub start: Point,
        "current" => pub current: Point,
        "view" => pub view: Option<ViewEnv>,
        "free" => pub free: Option<bool>,
    }
}

js_object! {
    pub struct GroupDraft {
        "op" => pub op: String,
        "ids" => pub ids: Vec<Id>,
        "handle" => pub handle: String,
        "anchor" => pub anchor: Point,
        "base" => pub base: Rect,
        "current" => pub current: Rect,
        "view" => pub view: Option<ViewEnv>,
    }
}

js_object! {
    pub struct CaptionDraft {
        "id" => pub id: Id,
        "start" => pub start: Point,
        "delta" => pub delta: Point,
    }
}

js_object! {
    pub struct LeaderDraft {
        "id" => pub id: Id,
        "start" => pub start: Point,
        "delta" => pub delta: f64,
    }
}

js_object! {
    pub struct MarqueeDraft {
        "page" => pub page: PageRef,
        "from" => pub from: Point,
        "to" => pub to: Point,
    }
}

js_union! {
    /// The gesture in progress (`Draft`).
    pub enum Draft ("kind") {
        "create-rect" => CreateRect(CreateRect),
        "create-distance" => CreateDistance(CreateDistance),
        "create-line" => CreateLine(CreateLine),
        "create-poly" => CreatePoly(CreatePoly),
        "create-ink" => CreateInk(CreateInk),
        "create-callout" => CreateCallout(CreateCallout),
        "move" => Move(MoveDraft),
        "handle" => Handle(HandleDraft),
        "rotate" => Rotate(RotateDraft),
        "group" => Group(GroupDraft),
        "caption" => Caption(CaptionDraft),
        "leader" => Leader(LeaderDraft),
        "marquee" => Marquee(MarqueeDraft),
    }
}

impl Draft {
    /// A draft that creates a record (`draft.kind.startsWith('create-')`,
    /// `view.ts`): one of the six `create-*` drafts. Read from the variant,
    /// not by printing the draft (review finding N10).
    pub fn is_create(&self) -> bool {
        matches!(
            self,
            Draft::CreateRect(_)
                | Draft::CreateDistance(_)
                | Draft::CreateLine(_)
                | Draft::CreatePoly(_)
                | Draft::CreateInk(_)
                | Draft::CreateCallout(_)
        )
    }

    /// The page a draft that has one (`'page' in draft`) is on.
    pub fn page(&self) -> Option<&PageRef> {
        match self {
            Draft::CreateRect(d) => Some(&d.page),
            Draft::CreateDistance(d) => Some(&d.page),
            Draft::CreateLine(d) => Some(&d.page),
            Draft::CreatePoly(d) => Some(&d.page),
            Draft::CreateInk(d) => Some(&d.page),
            Draft::CreateCallout(d) => Some(&d.page),
            Draft::Marquee(d) => Some(&d.page),
            _ => None,
        }
    }

    /// The ids a gesture in progress works on.
    pub fn ids(&self) -> Vec<&Id> {
        match self {
            Draft::Move(d) => d.ids.iter().collect(),
            Draft::Rotate(d) => d.ids.iter().collect(),
            Draft::Group(d) => d.ids.iter().collect(),
            Draft::Handle(d) => vec![&d.id],
            Draft::Caption(d) => vec![&d.id],
            Draft::Leader(d) => vec![&d.id],
            _ => Vec::new(),
        }
    }
}

js_object! {
    /// A live text-markup preview, per page (`MarkupPreview`).
    pub struct MarkupPreview {
        "subtype" => pub subtype: KindName,
        "preset" => pub preset: String,
        "byPage" => pub by_page: Record<Vec<Quad>>,
    }
}

/// Which annotations people may change by hand (`EditableKinds`).
#[derive(Clone, Debug, PartialEq)]
pub enum EditableKinds {
    All(bool),
    Kinds(Vec<String>),
}

impl Json for EditableKinds {
    fn from_json(value: Option<&Value>) -> Result<Self, JsonError> {
        match value {
            Some(Value::Bool(b)) => Ok(EditableKinds::All(*b)),
            other => Vec::<String>::from_json(other).map(EditableKinds::Kinds),
        }
    }
    fn to_json(&self) -> Option<Value> {
        match self {
            EditableKinds::All(b) => Some(Value::Bool(*b)),
            EditableKinds::Kinds(list) => list.to_json(),
        }
    }
}

/// The records a gesture works on, by id, in insertion order (`byId`).
pub type ById = IndexMap<Id, Rc<ModelAnnotation>>;

/// What every transition and read takes: the session and the records it
/// works on (`Model = Session & AnnotationView`). The members that stand
/// for JavaScript references (arrays and objects whose identity
/// `sameSession` checks) are `Rc`s, so a transition that keeps one keeps
/// its identity.
#[derive(Clone, Debug)]
pub struct Model {
    pub selected: Rc<Vec<Id>>,
    pub hovered: Option<Id>,
    pub draft: Option<Rc<Draft>>,
    pub preview: Option<Rc<MarkupPreview>>,
    pub object_numbers: Rc<Vec<f64>>,
    pub defaults: Rc<IndexMap<String, FieldValues>>,
    pub hit_margin: f64,
    pub editing: Option<Id>,
    pub snap: Rc<SnapSettings>,
    pub editable: Rc<EditableKinds>,
    pub by_id: Rc<ById>,
    pub order: Rc<Vec<Id>>,
}

/// The session a transition returns (`Session`).
#[derive(Clone, Debug)]
pub struct Session {
    pub selected: Rc<Vec<Id>>,
    pub hovered: Option<Id>,
    pub draft: Option<Rc<Draft>>,
    pub preview: Option<Rc<MarkupPreview>>,
    pub object_numbers: Rc<Vec<f64>>,
    pub defaults: Rc<IndexMap<String, FieldValues>>,
    pub hit_margin: f64,
    pub editing: Option<Id>,
    pub snap: Rc<SnapSettings>,
    pub editable: Rc<EditableKinds>,
}

fn opt_rc_eq<T>(a: &Option<Rc<T>>, b: &Option<Rc<T>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

impl Session {
    /// `sameSession`: every field the same value, arrays and objects by identity.
    pub fn same(&self, other: &Session) -> bool {
        Rc::ptr_eq(&self.selected, &other.selected)
            && self.hovered == other.hovered
            && opt_rc_eq(&self.draft, &other.draft)
            && opt_rc_eq(&self.preview, &other.preview)
            && Rc::ptr_eq(&self.object_numbers, &other.object_numbers)
            && Rc::ptr_eq(&self.defaults, &other.defaults)
            && (self.hit_margin == other.hit_margin
                || (self.hit_margin.is_nan() && other.hit_margin.is_nan()))
            && self.editing == other.editing
            && Rc::ptr_eq(&self.snap, &other.snap)
            && match (&*self.editable, &*other.editable) {
                (EditableKinds::All(a), EditableKinds::All(b)) => a == b,
                _ => Rc::ptr_eq(&self.editable, &other.editable),
            }
    }

    /// The session as the TypeScript object prints it.
    pub fn to_object(&self) -> Object {
        let mut out = Object::new();
        out.insert("selected".into(), self.selected.json());
        out.insert("hovered".into(), self.hovered.clone().map_or(Value::Null, Value::String));
        out.insert("draft".into(), self.draft.as_ref().map_or(Value::Null, |d| d.json()));
        out.insert("preview".into(), self.preview.as_ref().map_or(Value::Null, |p| p.json()));
        out.insert("objectNumbers".into(), self.object_numbers.json());
        let defaults: Object =
            self.defaults.iter().map(|(k, v)| (k.clone(), Value::Object(v.clone()))).collect();
        out.insert("defaults".into(), Value::Object(defaults));
        out.insert("hitMargin".into(), json::num(self.hit_margin));
        out.insert("editing".into(), self.editing.clone().map_or(Value::Null, Value::String));
        out.insert("snap".into(), self.snap.json());
        out.insert("editable".into(), self.editable.json());
        out
    }
}

impl Model {
    pub fn session(&self) -> Session {
        Session {
            selected: self.selected.clone(),
            hovered: self.hovered.clone(),
            draft: self.draft.clone(),
            preview: self.preview.clone(),
            object_numbers: self.object_numbers.clone(),
            defaults: self.defaults.clone(),
            hit_margin: self.hit_margin,
            editing: self.editing.clone(),
            snap: self.snap.clone(),
            editable: self.editable.clone(),
        }
    }

    /// `{ ...session, byId, order }`.
    pub fn of(session: Session, by_id: Rc<ById>, order: Rc<Vec<Id>>) -> Model {
        Model {
            selected: session.selected,
            hovered: session.hovered,
            draft: session.draft,
            preview: session.preview,
            object_numbers: session.object_numbers,
            defaults: session.defaults,
            hit_margin: session.hit_margin,
            editing: session.editing,
            snap: session.snap,
            editable: session.editable,
            by_id,
            order,
        }
    }

    pub fn record(&self, id: &str) -> Option<&ModelAnnotation> {
        self.by_id.get(id).map(|r| &**r)
    }

    pub fn is_selected(&self, id: &str) -> bool {
        self.selected.iter().any(|s| s == id)
    }

    pub fn draft(&self) -> Option<&Draft> {
        self.draft.as_deref()
    }

    /// The model with another selection (`{ ...model, selected }`).
    pub fn with_selected(&self, selected: Vec<Id>) -> Model {
        Model { selected: Rc::new(selected), ..self.clone() }
    }

    /// The model with another draft.
    pub fn with_draft(&self, draft: Option<Draft>) -> Model {
        Model { draft: draft.map(Rc::new), ..self.clone() }
    }
}

fn read_member<T: Json>(object: &Object, key: &str) -> Result<T, JsonError> {
    T::from_json(object.get(key)).map_err(|e| e.at(key))
}

impl Json for Model {
    fn from_json(value: Option<&Value>) -> Result<Self, JsonError> {
        let object =
            value.and_then(Value::as_object).ok_or_else(|| JsonError::new("not a model"))?;
        let defaults: IndexMap<String, FieldValues> =
            match object.get("defaults").and_then(Value::as_object) {
                Some(map) => map
                    .iter()
                    .map(|(k, v)| {
                        Ok((
                            k.clone(),
                            v.as_object().cloned().ok_or_else(|| JsonError::new("defaults"))?,
                        ))
                    })
                    .collect::<Result<_, JsonError>>()?,
                None => IndexMap::new(),
            };
        let mut by_id = ById::new();
        if let Some(map) = object.get("byId").and_then(Value::as_object) {
            for (id, record) in map {
                by_id.insert(
                    id.clone(),
                    Rc::new(ModelAnnotation::read(record).map_err(|e| e.at(id))?),
                );
            }
        }
        Ok(Model {
            selected: Rc::new(read_member(object, "selected")?),
            hovered: read_member::<Nullable<String>>(object, "hovered")
                .map(Nullable::into_option)
                .unwrap_or(None),
            draft: read_member::<Nullable<Draft>>(object, "draft")?.into_option().map(Rc::new),
            preview: read_member::<Nullable<MarkupPreview>>(object, "preview")
                .map(Nullable::into_option)
                .unwrap_or(None)
                .map(Rc::new),
            object_numbers: Rc::new(read_member(object, "objectNumbers").unwrap_or_default()),
            defaults: Rc::new(defaults),
            hit_margin: read_member(object, "hitMargin")?,
            editing: read_member::<Nullable<String>>(object, "editing")
                .map(Nullable::into_option)
                .unwrap_or(None),
            snap: Rc::new(read_member(object, "snap")?),
            editable: Rc::new(read_member(object, "editable").unwrap_or(EditableKinds::All(true))),
            by_id: Rc::new(by_id),
            order: Rc::new(read_member(object, "order")?),
        })
    }
    fn to_json(&self) -> Option<Value> {
        let mut out = self.session().to_object();
        let by_id: Object = self.by_id.iter().map(|(k, v)| (k.clone(), v.json())).collect();
        out.insert("byId".into(), Value::Object(by_id));
        out.insert("order".into(), self.order.json());
        Some(Value::Object(out))
    }
}

// ── results and effects ─────────────────────────────────────────────────────

/// What one message changed in the records (`ChangeSet`).
#[derive(Clone, Debug, Default)]
pub struct ChangeSet {
    pub put: Vec<Rc<ModelAnnotation>>,
    pub drop: Vec<Id>,
    pub patches: IndexMap<Id, AnnotationPatch>,
}

impl ChangeSet {
    pub fn is_empty(&self) -> bool {
        self.put.is_empty() && self.drop.is_empty()
    }

    pub fn to_object(&self) -> Object {
        let patches: Object =
            self.patches.iter().map(|(k, v)| (k.clone(), Value::Object(v.clone()))).collect();
        json::object([
            ("put", Value::Array(self.put.iter().map(|r| r.json()).collect())),
            ("drop", self.drop.json()),
            ("patches", Value::Object(patches)),
        ])
    }
}

js_object! {
    pub struct CapturedEffect {
        "tool" => pub tool: String,
        "page" => pub page: PageRef,
        "geometry" => pub geometry: Shape,
    }
}

js_object! {
    pub struct CreateEffect {
        "id" => pub id: Id,
        "draft" => pub draft: AnnotationDraft,
    }
}

js_object! {
    pub struct PatchEffect {
        "id" => pub id: Id,
        "patch" => pub patch: AnnotationPatch,
    }
}

js_object! {
    pub struct IdEffect {
        "id" => pub id: Id,
    }
}

js_object! {
    pub struct SyncLinkEffect {
        "id" => pub id: Id,
        "target" => pub target: Value,
    }
}

js_union! {
    /// Engine work a message asks for (`Effect`).
    pub enum Effect ("type") {
        "captured" => Captured(CapturedEffect),
        "create" => Create(CreateEffect),
        "patch" => Patch(PatchEffect),
        "text" => Text(IdEffect),
        "syncLink" => SyncLink(SyncLinkEffect),
        "delete" => Delete(IdEffect),
    }
}

impl Effect {
    pub fn create(id: &str, draft: AnnotationDraft) -> Effect {
        Effect::Create(CreateEffect { id: id.into(), draft, order: Default::default() })
    }
    pub fn patch(id: &str, patch: AnnotationPatch) -> Effect {
        Effect::Patch(PatchEffect { id: id.into(), patch, order: Default::default() })
    }
    pub fn text(id: &str) -> Effect {
        Effect::Text(IdEffect { id: id.into(), order: Default::default() })
    }
    pub fn delete(id: &str) -> Effect {
        Effect::Delete(IdEffect { id: id.into(), order: Default::default() })
    }
    pub fn sync_link(id: &str, target: Value) -> Effect {
        Effect::SyncLink(SyncLinkEffect { id: id.into(), target, order: Default::default() })
    }
}

/// One message's result (`UpdateResult`).
#[derive(Clone, Debug)]
pub struct UpdateResult {
    pub session: Session,
    pub change: ChangeSet,
    pub effects: Vec<Effect>,
}

// ── view output ─────────────────────────────────────────────────────────────

js_object! {
    /// A draggable handle (`Handle`).
    pub struct Handle {
        "id" => pub id: String,
        "at" => pub at: Point,
        /// Absent where the TypeScript's cursor table has none (a box turned
        /// by a non-finite angle).
        "cursor" => pub cursor: Option<Cursor>,
    }
}

impl Handle {
    pub fn new(id: impl Into<String>, at: Point, cursor: impl Into<String>) -> Self {
        Handle::maybe(id, at, Some(cursor.into()))
    }
    /// A handle whose cursor may be absent.
    pub fn maybe(id: impl Into<String>, at: Point, cursor: Option<Cursor>) -> Self {
        Handle { id: id.into(), at, cursor, order: Default::default() }
    }
}

js_object! {
    /// Chrome grab-zone geometry (`ChromeGeometry`).
    pub struct ChromeGeometry {
        "handleTol" => pub handle_tol: f64,
        "knobTol" => pub knob_tol: f64,
        "knobOffset" => pub knob_offset: f64,
        "rotationHandle" => pub rotation_handle: Option<bool>,
    }
}

js_object! {
    pub struct RectNode {
        "rect" => pub rect: Rect,
    }
}

js_object! {
    pub struct LineNode {
        "a" => pub a: Point,
        "b" => pub b: Point,
    }
}

js_object! {
    pub struct PolyNode {
        "points" => pub points: Vec<Point>,
        "closed" => pub closed: bool,
    }
}

js_object! {
    pub struct PathNode {
        "d" => pub d: String,
    }
}

js_union! {
    /// The geometry-only draw vocabulary (`RenderNode`).
    pub enum RenderNode ("kind") {
        "rect" => Rect(RectNode),
        "ellipse" => Ellipse(RectNode),
        "line" => Line(LineNode),
        "poly" => Poly(PolyNode),
        "path" => Path(PathNode),
    }
}

impl RenderNode {
    pub fn rect(rect: Rect) -> Self {
        RenderNode::Rect(RectNode { rect, order: Default::default() })
    }
    pub fn ellipse(rect: Rect) -> Self {
        RenderNode::Ellipse(RectNode { rect, order: Default::default() })
    }
    pub fn line(a: Point, b: Point) -> Self {
        RenderNode::Line(LineNode { a, b, order: Default::default() })
    }
    pub fn poly(points: Vec<Point>, closed: bool) -> Self {
        RenderNode::Poly(PolyNode { points, closed, order: Default::default() })
    }
    pub fn path(d: String) -> Self {
        RenderNode::Path(PathNode { d, order: Default::default() })
    }
}

js_object! {
    /// How to paint one node (`Paint`); an absent member means none.
    #[derive(Default)]
    pub struct Paint {
        "fill" => pub fill: Option<Value>,
        "stroke" => pub stroke: Option<Value>,
        "width" => pub width: Option<f64>,
        "opacity" => pub opacity: Option<Value>,
        "dash" => pub dash: Option<Vec<f64>>,
        "blend" => pub blend: Option<String>,
        "lineCap" => pub line_cap: Option<String>,
        "join" => pub join: Option<String>,
        "fillRule" => pub fill_rule: Option<String>,
    }
}

/// A font program a scene's text draws in (`SceneFontProgram`): mounted
/// under its own family, weight and style. The core names no font data of
/// its own; whoever draws text (the form painter) brings its programs.
pub trait SceneFontProgram: fmt::Debug {
    /// The program's members as its object prints them, `load` aside.
    fn members(&self) -> Object;
    fn family(&self) -> &str;
    fn weight(&self) -> f64;
    /// `'normal'` or `'italic'`.
    fn style(&self) -> &str;
    /// The program's bytes.
    fn load(&self) -> &[u8];
}

/// A shared font program.
#[derive(Clone, Debug)]
pub struct FontProgramRef(pub Rc<dyn SceneFontProgram>);

impl PartialEq for FontProgramRef {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0) || self.0.members() == other.0.members()
    }
}

impl Json for FontProgramRef {
    fn from_json(_: Option<&Value>) -> Result<Self, JsonError> {
        Err(JsonError::new("a font program doesn't read from JSON"))
    }
    fn to_json(&self) -> Option<Value> {
        Some(Value::Object(self.0.members()))
    }
}

js_object! {
    pub struct ShapeScene {
        "rect" => pub rect: Option<Rect>,
        "a" => pub a: Option<Point>,
        "b" => pub b: Option<Point>,
        "points" => pub points: Option<Vec<Point>>,
        "closed" => pub closed: Option<bool>,
        "d" => pub d: Option<String>,
        "paint" => pub paint: Paint,
        "clip" => pub clip: Option<Rect>,
    }
}

js_object! {
    /// Painted text (`SceneNode` of kind `text`).
    pub struct TextScene {
        "at" => pub at: Point,
        "text" => pub text: String,
        "fontSize" => pub font_size: f64,
        "fontFamily" => pub font_family: Option<String>,
        "rotation" => pub rotation: Option<f64>,
        "paint" => pub paint: Paint,
        "clip" => pub clip: Option<Rect>,
    }
}

js_object! {
    /// Text placed character by character (`SceneNode` of kind `glyphs`).
    pub struct GlyphsScene {
        "at" => pub at: Point,
        "text" => pub text: String,
        "offsets" => pub offsets: Vec<f64>,
        "fontSize" => pub font_size: f64,
        "fontFamily" => pub font_family: String,
        "fontWeight" => pub font_weight: Option<f64>,
        "fontStyle" => pub font_style: Option<String>,
        "rotation" => pub rotation: Option<f64>,
        "program" => pub program: Option<FontProgramRef>,
        "paint" => pub paint: Paint,
        "clip" => pub clip: Option<Rect>,
    }
}

js_union! {
    /// A fully painted draw node (`SceneNode`): a renderer maps each to one element.
    pub enum SceneNode ("kind") {
        "rect" => Rect(ShapeScene),
        "ellipse" => Ellipse(ShapeScene),
        "line" => Line(ShapeScene),
        "poly" => Poly(ShapeScene),
        "path" => Path(ShapeScene),
        "text" => Text(TextScene),
        "glyphs" => Glyphs(GlyphsScene),
    }
}

impl SceneNode {
    /// `{ ...node, paint }`: a geometry node painted.
    pub fn painted(node: &RenderNode, paint: Paint) -> SceneNode {
        let mut scene = ShapeScene {
            rect: None,
            a: None,
            b: None,
            points: None,
            closed: None,
            d: None,
            paint,
            clip: None,
            order: Default::default(),
        };
        let keys: &[&str] = match node {
            RenderNode::Rect(n) | RenderNode::Ellipse(n) => {
                scene.rect = Some(n.rect);
                &["rect", "paint"]
            }
            RenderNode::Line(n) => {
                scene.a = Some(n.a);
                scene.b = Some(n.b);
                &["a", "b", "paint"]
            }
            RenderNode::Poly(n) => {
                scene.points = Some(n.points.clone());
                scene.closed = Some(n.closed);
                &["points", "closed", "paint"]
            }
            RenderNode::Path(n) => {
                scene.d = Some(n.d.clone());
                &["d", "paint"]
            }
        };
        let scene = scene.ordered(keys);
        match node {
            RenderNode::Rect(_) => SceneNode::Rect(scene),
            RenderNode::Ellipse(_) => SceneNode::Ellipse(scene),
            RenderNode::Line(_) => SceneNode::Line(scene),
            RenderNode::Poly(_) => SceneNode::Poly(scene),
            RenderNode::Path(_) => SceneNode::Path(scene),
        }
    }

    /// A path node.
    pub fn path(d: String, paint: Paint) -> SceneNode {
        SceneNode::painted(&RenderNode::path(d), paint)
    }

    pub fn paint(&self) -> &Paint {
        match self {
            SceneNode::Rect(s)
            | SceneNode::Ellipse(s)
            | SceneNode::Line(s)
            | SceneNode::Poly(s)
            | SceneNode::Path(s) => &s.paint,
            SceneNode::Text(t) => &t.paint,
            SceneNode::Glyphs(g) => &g.paint,
        }
    }

    pub fn clip(&self) -> Option<Rect> {
        match self {
            SceneNode::Rect(s)
            | SceneNode::Ellipse(s)
            | SceneNode::Line(s)
            | SceneNode::Poly(s)
            | SceneNode::Path(s) => s.clip,
            SceneNode::Text(t) => t.clip,
            SceneNode::Glyphs(g) => g.clip,
        }
    }
}

js_object! {
    pub struct OutlineNode {
        "rect" => pub rect: Rect,
    }
}

js_object! {
    pub struct ObbNode {
        "corners" => pub corners: Vec<Point>,
        "angle" => pub angle: f64,
    }
}

js_object! {
    pub struct HandleNode {
        "at" => pub at: Point,
        "cursor" => pub cursor: Option<Cursor>,
        "rot" => pub rot: Option<f64>,
        "role" => pub role: String,
        "active" => pub active: bool,
    }
}

js_object! {
    pub struct KnobNode {
        "at" => pub at: Point,
        "from" => pub from: Point,
    }
}

js_object! {
    pub struct GuideNode {
        "axis" => pub axis: String,
        "at" => pub at: f64,
        "lo" => pub lo: f64,
        "hi" => pub hi: f64,
    }
}

js_object! {
    pub struct GuideLine {
        "a" => pub a: Point,
        "b" => pub b: Point,
        "role" => pub role: String,
    }
}

js_object! {
    pub struct RotateGuidesNode {
        "center" => pub center: Point,
        "angle" => pub angle: f64,
        "lines" => pub lines: Vec<GuideLine>,
    }
}

js_union! {
    /// Selection chrome (`ChromeNode`).
    pub enum ChromeNode ("kind") {
        "outline" => Outline(OutlineNode),
        "obb" => Obb(ObbNode),
        "handle" => Handle(HandleNode),
        "rotate-knob" => RotateKnob(KnobNode),
        "guide" => Guide(GuideNode),
        "rotate-guides" => RotateGuides(RotateGuidesNode),
        "marquee" => Marquee(OutlineNode),
    }
}

js_object! {
    /// A redaction's label, as its hover preview draws it.
    pub struct RedactLabel {
        "text" => pub text: String,
        "repeat" => pub repeat: Option<Value>,
    }
}

js_object! {
    /// Where a render item draws: its box, turn and scale.
    pub struct Frame {
        "box" => pub box_: Rect,
        "rotation" => pub rotation: f64,
        "scale" => pub scale: f64,
    }
}

js_object! {
    /// The engine's raster inside a frame.
    pub struct Raster {
        "box" => pub box_: Rect,
        "rotation" => pub rotation: f64,
    }
}

js_strings! {
    /// How a render item paints (`RenderItem['source']`).
    pub enum ItemSource {
        "baked" => Baked,
        "vector" => Vector,
        "draft" => Draft,
        "ghost" => Ghost,
    }
}

impl From<Source> for ItemSource {
    fn from(source: Source) -> Self {
        match source {
            Source::Baked => ItemSource::Baked,
            Source::Vector => ItemSource::Vector,
        }
    }
}

/// A lent record's drawing, made the first time it's asked for.
#[derive(Clone, Default)]
pub struct LentScene(pub Option<Rc<dyn Fn() -> Vec<SceneNode>>>);

impl fmt::Debug for LentScene {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.0.is_some() { "LentScene(..)" } else { "LentScene(None)" })
    }
}

impl PartialEq for LentScene {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        }
    }
}

impl Json for LentScene {
    fn from_json(_: Option<&Value>) -> Result<Self, JsonError> {
        Ok(LentScene(None))
    }
    fn to_json(&self) -> Option<Value> {
        self.0.as_ref().map(|_| {
            let mut marker = Object::new();
            marker.insert("$function".into(), Value::Bool(true));
            Value::Object(marker)
        })
    }
}

js_object! {
    /// Per-annotation render data (`RenderItem`).
    pub struct RenderItem {
        "id" => pub id: Id,
        "ref" => pub reference: Option<Value>,
        "annotation" => pub annotation: Option<Annotation>,
        "subtype" => pub subtype: KindName,
        "geometry" => pub geometry: Shape,
        "box" => pub box_: Rect,
        "apBox" => pub ap_box: Option<Rect>,
        "style" => pub style: Style,
        "text" => pub text: Option<TextStyle>,
        "icon" => pub icon: Option<Value>,
        "label" => pub label: Option<RedactLabel>,
        "measure" => pub measure: Option<Object>,
        "source" => pub source: ItemSource,
        "pictureVersion" => pub picture_version: Option<f64>,
        "lentScene" => pub lent_scene: LentScene,
        "ghostOpacity" => pub ghost_opacity: Option<f64>,
        "selected" => pub selected: bool,
        "hovered" => pub hovered: Option<bool>,
        "rot" => pub rot: Option<f64>,
        "apRot" => pub ap_rot: Option<f64>,
        "blend" => pub blend: Option<String>,
        "frame" => pub frame: Option<Frame>,
        "raster" => pub raster: Option<Nullable<Raster>>,
    }
}

js_object! {
    pub struct AnnotTarget {
        "id" => pub id: Id,
    }
}

js_object! {
    pub struct HandleTarget {
        "id" => pub id: Id,
        "handle" => pub handle: String,
        "cursor" => pub cursor: Option<Cursor>,
    }
}

js_object! {
    pub struct RotateTarget {
        "ids" => pub ids: Vec<Id>,
        "pivot" => pub pivot: Point,
    }
}

js_object! {
    pub struct GroupHandleTarget {
        "ids" => pub ids: Vec<Id>,
        "handle" => pub handle: String,
        "cursor" => pub cursor: Option<Cursor>,
        "box" => pub box_: Rect,
    }
}

js_object! {
    pub struct EmptyTarget {}
}

js_union! {
    /// What's under a page point (`Target`).
    pub enum Target ("kind") {
        "handle" => Handle(HandleTarget),
        "rotate" => Rotate(RotateTarget),
        "group-handle" => GroupHandle(GroupHandleTarget),
        "annot" => Annot(AnnotTarget),
        "empty" => Empty(EmptyTarget),
    }
}

impl Target {
    pub fn empty() -> Target {
        Target::Empty(EmptyTarget { order: Default::default() })
    }
    pub fn annot(id: &str) -> Target {
        Target::Annot(AnnotTarget { id: id.into(), order: Default::default() })
    }
}

js_object! {
    /// A rotation in progress, for the badge (`RotationAnchor`).
    pub struct RotationAnchor {
        "page" => pub page: PageRef,
        "at" => pub at: Point,
        "angle" => pub angle: f64,
    }
}

js_object! {
    /// Anchor for a poly creation draft's controls (`CreationDraftAnchor`).
    pub struct CreationDraftAnchor {
        "kind" => pub kind: String,
        "subtype" => pub subtype: String,
        "page" => pub page: PageRef,
        "bounds" => pub bounds: Rect,
        "pointCount" => pub point_count: f64,
        "minPoints" => pub min_points: f64,
        "canFinish" => pub can_finish: bool,
    }
}

js_object! {
    /// Settings for straightening a freehand stroke (`InkStraightenOptions`).
    pub struct InkStraightenOptions {
        "deviationThreshold" => pub deviation_threshold: f64,
        "axisSnapDegrees" => pub axis_snap_degrees: f64,
    }
}

js_object! {
    /// A box placement: a box and the turn an upright tool gives it.
    pub struct BoxPlacement {
        "rect" => pub rect: Rect,
        "rot" => pub rot: f64,
    }
}

js_object! {
    /// A segment placement.
    pub struct SegmentPlacement {
        "a" => pub a: Point,
        "b" => pub b: Point,
    }
}

js_union! {
    /// Where a create gesture puts what it makes (`Placement`).
    pub enum Placement ("kind") {
        "box" => Box(BoxPlacement),
        "segment" => Segment(SegmentPlacement),
    }
}

impl Placement {
    pub fn boxed(rect: Rect, rot: f64) -> Placement {
        Placement::Box(BoxPlacement { rect, rot, order: Default::default() })
    }
    pub fn segment(a: Point, b: Point) -> Placement {
        Placement::Segment(SegmentPlacement { a, b, order: Default::default() })
    }
}

/// The input of a pointer message (`PointerInput`).
#[derive(Clone, Debug, Default)]
pub struct PointerInput {
    pub page: Option<PageRef>,
    pub point: Point,
    pub shift: bool,
    pub finish: Option<bool>,
    pub page_box: Option<Rect>,
    pub chrome: Option<ChromeGeometry>,
    pub display_rotation: Option<f64>,
    pub scale: Option<f64>,
    pub zoom: Option<f64>,
    pub upright: Option<bool>,
    pub inert: Option<crate::js::IdSet>,
}

impl PointerInput {
    pub fn page(&self) -> PageRef {
        self.page.clone().unwrap_or_else(|| PageRef::new(f64::NAN))
    }

    pub fn page_number(&self) -> f64 {
        self.page.as_ref().map_or(f64::NAN, |p| p.object_number)
    }

    pub fn read(value: &Value) -> Result<Self, JsonError> {
        let object = value.as_object().ok_or_else(|| JsonError::new("not a pointer input"))?;
        let inert = object
            .get("inert")
            .filter(|v| !json::undefined(Some(v)))
            .map(crate::js::IdSet::of_value);
        Ok(PointerInput {
            page: read_member(object, "page")?,
            point: read_member(object, "point")?,
            shift: read_member::<Option<bool>>(object, "shift")?.unwrap_or(false),
            finish: read_member(object, "finish")?,
            page_box: read_member(object, "pageBox")?,
            chrome: read_member(object, "chrome")?,
            display_rotation: read_member(object, "displayRotation")?,
            scale: read_member(object, "scale")?,
            zoom: read_member(object, "zoom")?,
            upright: read_member(object, "upright")?,
            inert,
        })
    }
}

/// A pointer phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Down,
    Move,
    Up,
}

impl Phase {
    fn read(text: &str) -> Result<Phase, JsonError> {
        match text {
            "down" => Ok(Phase::Down),
            "move" => Ok(Phase::Move),
            "up" => Ok(Phase::Up),
            other => Err(JsonError::new(format!("not a phase: {other}"))),
        }
    }
}

/// The messages `update` takes (`Message`).
#[derive(Clone, Debug)]
pub enum Message {
    EditPointer {
        phase: Phase,
        input: PointerInput,
    },
    MarqueePointer {
        phase: Phase,
        input: PointerInput,
    },
    CreatePointer(Box<CreatePointerMessage>),
    FinishInkDraft,
    FinishCreationDraft,
    CreateCaret {
        page: PageRef,
        anchor: TextEndAnchor,
        flags: Option<Object>,
    },
    CreateReplaceText {
        page: PageRef,
        quads: Vec<Quad>,
        anchor: TextEndAnchor,
        preset: Option<String>,
    },
    CreateMarkup {
        subtype: KindName,
        page: PageRef,
        quads: Vec<Quad>,
        preset: Option<String>,
        flags: Option<Object>,
    },
    SetMarkupPreview {
        subtype: KindName,
        quads_by_page: Record<Vec<Quad>>,
        preset: Option<String>,
    },
    ClearMarkupPreview,
    Deselect {
        ids: Option<Vec<Id>>,
    },
    Hover {
        id: Option<Id>,
    },
    Select {
        ids: Vec<Id>,
        add: Option<bool>,
    },
    SetFields {
        patches: IndexMap<Id, FieldValues>,
    },
    SetTextFormat {
        format: String,
        on: bool,
    },
    SetLink {
        target: Value,
    },
    SetFlags {
        patch: Object,
        ids: Option<Vec<Id>>,
    },
    SetDefaults {
        preset: String,
        patch: FieldValues,
    },
    SetSnap {
        patch: Object,
    },
    RotateSelection {
        degrees: f64,
    },
    ResetRotation,
    Delete,
    Cancel,
    Forget {
        ids: Vec<Id>,
    },
    BeginTextEdit {
        id: Id,
    },
    SetText {
        id: Id,
        text: String,
    },
    SetRichText {
        id: Id,
        doc: Object,
    },
    EndTextEdit,
}

/// A `createPointer` message.
#[derive(Clone, Debug)]
pub struct CreatePointerMessage {
    pub measure: Option<Object>,
    pub capture: Option<String>,
    pub phase: Phase,
    pub subtype: KindName,
    pub preset: Option<String>,
    pub intent: Option<String>,
    pub click_create: Option<ClickPolicy>,
    pub flags: Option<Object>,
    pub defer_ink_commit: Option<bool>,
    pub straighten_ink: Option<InkStraightenOptions>,
    pub input: PointerInput,
}

impl Message {
    pub fn read(value: &Value) -> Result<Message, JsonError> {
        let object = value.as_object().ok_or_else(|| JsonError::new("not a message"))?;
        let ty = object.get("type").and_then(Value::as_str).unwrap_or("");
        let phase = || -> Result<Phase, JsonError> {
            Phase::read(&read_member::<String>(object, "phase")?)
        };
        let input = || -> Result<PointerInput, JsonError> {
            PointerInput::read(object.get("in").ok_or_else(|| JsonError::new("in: missing"))?)
        };
        let fields = |key: &str| -> Result<IndexMap<Id, FieldValues>, JsonError> {
            let map = object
                .get(key)
                .and_then(Value::as_object)
                .ok_or_else(|| JsonError::new(format!("{key}: missing")))?;
            map.iter()
                .map(|(k, v)| {
                    Ok((k.clone(), v.as_object().cloned().ok_or_else(|| JsonError::new("patch"))?))
                })
                .collect()
        };
        Ok(match ty {
            "editPointer" => Message::EditPointer { phase: phase()?, input: input()? },
            "marqueePointer" => Message::MarqueePointer { phase: phase()?, input: input()? },
            "createPointer" => Message::CreatePointer(Box::new(CreatePointerMessage {
                measure: read_member(object, "measure")?,
                capture: read_member(object, "capture")?,
                phase: phase()?,
                subtype: read_member(object, "subtype")?,
                preset: read_member(object, "preset")?,
                intent: read_member(object, "intent")?,
                click_create: read_member(object, "clickCreate")?,
                flags: read_member(object, "flags")?,
                defer_ink_commit: read_member(object, "deferInkCommit")?,
                straighten_ink: read_member(object, "straightenInk")?,
                input: input()?,
            })),
            "finishInkDraft" => Message::FinishInkDraft,
            "finishCreationDraft" => Message::FinishCreationDraft,
            "createCaret" => Message::CreateCaret {
                page: read_member(object, "page")?,
                anchor: read_member(object, "anchor")?,
                flags: read_member(object, "flags")?,
            },
            "createReplaceText" => Message::CreateReplaceText {
                page: read_member(object, "page")?,
                quads: read_member(object, "quads")?,
                anchor: read_member(object, "anchor")?,
                preset: read_member(object, "preset")?,
            },
            "createMarkup" => Message::CreateMarkup {
                subtype: read_member(object, "subtype")?,
                page: read_member(object, "page")?,
                quads: read_member(object, "quads")?,
                preset: read_member(object, "preset")?,
                flags: read_member(object, "flags")?,
            },
            "setMarkupPreview" => Message::SetMarkupPreview {
                subtype: read_member(object, "subtype")?,
                quads_by_page: read_member(object, "quadsByPage")?,
                preset: read_member(object, "preset")?,
            },
            "clearMarkupPreview" => Message::ClearMarkupPreview,
            "deselect" => Message::Deselect { ids: read_member(object, "ids")? },
            "hover" => Message::Hover {
                id: read_member::<Nullable<String>>(object, "id")
                    .map(Nullable::into_option)
                    .unwrap_or(None),
            },
            "select" => Message::Select {
                ids: read_member(object, "ids")?,
                add: read_member(object, "add")?,
            },
            "setFields" => Message::SetFields { patches: fields("patches")? },
            "setTextFormat" => Message::SetTextFormat {
                format: read_member(object, "format")?,
                on: read_member(object, "on")?,
            },
            "setLink" => {
                Message::SetLink { target: object.get("target").cloned().unwrap_or(Value::Null) }
            }
            "setFlags" => Message::SetFlags {
                patch: read_member(object, "patch")?,
                ids: read_member(object, "ids")?,
            },
            "setDefaults" => Message::SetDefaults {
                preset: read_member(object, "preset")?,
                patch: read_member(object, "patch")?,
            },
            "setSnap" => Message::SetSnap { patch: read_member(object, "patch")? },
            "rotateSelection" => {
                Message::RotateSelection { degrees: read_member(object, "degrees")? }
            }
            "resetRotation" => Message::ResetRotation,
            "delete" => Message::Delete,
            "cancel" => Message::Cancel,
            "forget" => Message::Forget { ids: read_member(object, "ids")? },
            "beginTextEdit" => Message::BeginTextEdit { id: read_member(object, "id")? },
            "setText" => Message::SetText {
                id: read_member(object, "id")?,
                text: read_member(object, "text")?,
            },
            "setRichText" => Message::SetRichText {
                id: read_member(object, "id")?,
                doc: read_member(object, "doc")?,
            },
            "endTextEdit" => Message::EndTextEdit,
            other => return Err(JsonError::new(format!("unknown message type {other:?}"))),
        })
    }
}
