//! Creating, changing and removing a form field and its widgets.
//!
//! G12. Ports the structural half of
//! `services/src/features/forms/FormMutator.ts` (`createField`,
//! `updateField`, `reorderCalculations`, `followCalculateScript`,
//! `deleteField`, `addWidget`, `detachWidget`, `assertPatchFitsFamily`,
//! `onStateOf`), `internal/authorWidget.ts`, `internal/fieldFlagBits.ts`,
//! `internal/fieldAttribution.ts` (`stampFieldCreation`, `writeFieldGroup`),
//! and the core rules they run: `core/src/forms/draft.ts`, `patch.ts`,
//! `placement.ts` (`placedWidgetOf`), `widgets.ts` (`fitWidgetToFamily`) and
//! `predict.ts` (`defaultEntryOfDraft`, `formFieldAfter`). The change ops of
//! [`crate::features::changes::fields`] run them.
//!
//! # A field and its widgets are born apart and joined
//!
//! `EPDFForm_CreateField` makes the FIELD: a dictionary in `/AcroForm
//! /Fields` with a name, a family and no geometry. What a reader sees on a
//! page is a WIDGET, which is an annotation, born through the annotation
//! plane (`EPDFPage_CreateAnnotRaw`). Adoption (`EPDFForm_AttachWidget`)
//! ties them together and draws the widget's appearance.
//!
//! # All of it, or none
//!
//! Everything a caller can get wrong is checked before the first write; a
//! failure after it aborts the layer transaction the write runs in, so a
//! refused draft creates nothing. Upstream runs every write in one layer
//! transaction (Task 8.30 ports that); until then each write here opens its
//! own when none is open.
//!
//! # The family-defining bits are not patchable
//!
//! `/Ff` bit 16 makes a radio, bit 17 a push button and bit 18 a combo box.
//! `EPDFForm_CreateField` sets them and the setter refuses them afterwards,
//! so [`FieldFlags`] does not name them.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use pdfium::{CreatableFieldFamily, Document};

use super::calculation::{
    OrderChange, read_calculation_order, same_order, write_calculation_order,
};
use super::snapshot::{
    FormFieldDto, FormFieldFamily, FormFieldKind, FormFieldOption, FormFieldRef, FormValueEntry,
    FormWidgetRef,
};
use super::write::{FormMutator, load_model, read_at, read_field_back, resolve};
use crate::access::OwnedDocument;
use crate::auth::scope::IdentityClaims;
use crate::cancel::CancelToken;
use crate::dto::Patch;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::actions::write::{
    FieldActionsPatch, WidgetActionsPatch, write_field_scripts, write_widget_actions,
};
use crate::features::annotation::write::context::AnnotationWriteContext;
use crate::features::annotation::write::widget::{
    WIDGET_STYLE_NAMES, WidgetStyle, apply_widget_style, read_style_field,
};
use crate::features::changes::transaction::in_layer_transaction;
use crate::geom::{PdfRect, PdfRotation, normalize_pdf_rect};
use crate::identity::{AnnotationRef, PageObjectNumber};
use crate::page_space::{Coordinates, PdfCoordinates};

/// The `/Ff` bits a draft or a patch may set (`FIELD_FLAG_BITS`).
///
/// Optional booleans rather than a word: `Some(true)` sets the bit,
/// `Some(false)` clears it, `None` leaves it alone. A word would have no way
/// to say the third thing, and a field's `/Ff` carries bits this build does
/// not model that a whole-word write would drop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FieldFlags {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_only: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_export: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multiline: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_toggle_to_off: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edit: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multi_select: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comb: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radios_in_unison: Option<bool>,
}

impl FieldFlags {
    /// The bits to set and the bits to clear (`flagMasks`).
    ///
    /// The shifts are 0-based and the spec's bit numbers are 1-based, which
    /// is where a table read straight off §12.7.4 goes wrong by one.
    pub fn masks(self) -> (u32, u32) {
        /// One bit: how to read it off the struct, and where it sits.
        type Bit = (fn(&FieldFlags) -> Option<bool>, u32);
        const BITS: [Bit; 10] = [
            (|f| f.read_only, 1 << 0),
            (|f| f.required, 1 << 1),
            (|f| f.no_export, 1 << 2),
            (|f| f.multiline, 1 << 12),
            (|f| f.password, 1 << 13),
            (|f| f.no_toggle_to_off, 1 << 14),
            (|f| f.edit, 1 << 18),
            (|f| f.multi_select, 1 << 21),
            (|f| f.comb, 1 << 24),
            (|f| f.radios_in_unison, 1 << 25),
        ];
        let mut set = 0;
        let mut clear = 0;
        for (read, bit) in BITS {
            match read(&self) {
                Some(true) => set |= bit,
                Some(false) => clear |= bit,
                None => {}
            }
        }
        (set, clear)
    }
}

/// One choice of a combo box or a list box at authoring time
/// (`FormFieldOptionInput`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldOptionInput {
    /// What a reader shows.
    pub label: String,
    /// What a value write and an export use.
    pub value: String,
}

/// The families a draft makes: every family but `unknown`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CreatableFamily {
    Text,
    Checkbox,
    Radio,
    Combobox,
    Listbox,
    Pushbutton,
    Signature,
}

impl CreatableFamily {
    fn native(self) -> CreatableFieldFamily {
        match self {
            Self::Text => CreatableFieldFamily::Text,
            Self::Checkbox => CreatableFieldFamily::Checkbox,
            Self::Radio => CreatableFieldFamily::Radio,
            Self::Combobox => CreatableFieldFamily::Combobox,
            Self::Listbox => CreatableFieldFamily::Listbox,
            Self::Pushbutton => CreatableFieldFamily::Pushbutton,
            Self::Signature => CreatableFieldFamily::Signature,
        }
    }

    pub fn family(self) -> FormFieldFamily {
        match self {
            Self::Text => FormFieldFamily::Text,
            Self::Checkbox => FormFieldFamily::Checkbox,
            Self::Radio => FormFieldFamily::Radio,
            Self::Combobox => FormFieldFamily::Combobox,
            Self::Listbox => FormFieldFamily::Listbox,
            Self::Pushbutton => FormFieldFamily::Pushbutton,
            Self::Signature => FormFieldFamily::Signature,
        }
    }
}

/// A default as a draft or a patch gives it: one string for a text field
/// or a dropdown, option values for a list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DefaultValueInput {
    One(String),
    Many(Vec<String>),
}

impl DefaultValueInput {
    fn values(&self) -> Vec<String> {
        match self {
            Self::One(value) => vec![value.clone()],
            Self::Many(values) => values.clone(),
        }
    }
}

/// Where a widget goes and how it looks (`WidgetPlacement<C>`): its page,
/// its place (`rect`, where it stands, or `box`, the frame its contents are
/// laid out in before the turn), its turn, a toggle's export value, its
/// actions, and the style fields a widget's update takes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", bound = "")]
pub struct WidgetPlacement<C: Coordinates = PdfCoordinates> {
    #[serde(with = "crate::identity::page_ref")]
    pub page: PageObjectNumber,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rect: Option<C::Rect>,
    #[serde(default, rename = "box", skip_serializing_if = "Option::is_none")]
    pub content_box: Option<C::Rect>,
    /// Degrees clockwise, a quarter turn; kept as sent so a turn that isn't
    /// one is the placement's refusal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub export_value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actions: Option<WidgetActionsPatch<C::Destination>>,
    /// The style fields, as sent ([`WIDGET_STYLE_NAMES`]).
    #[serde(flatten)]
    pub style: Map<String, Value>,
}

/// What `forms.create` takes (`FormFieldDraft<C>`). The worker does not
/// check a draft against its family's schema: a member another family has
/// is written, and the native write refuses what the field can't hold.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", bound = "")]
pub struct FormFieldDraft<C: Coordinates = PdfCoordinates> {
    pub family: CreatableFamily,
    /// The dotted fully qualified name. Missing ancestors are created and a
    /// sibling collision is refused.
    pub name: String,
    /// The `/Ff` bits, at the top level of the wire object.
    #[serde(flatten)]
    pub flags: FieldFlags,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alternate_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mapping_name: Option<String>,
    /// The group the field goes in; the session's own when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actions: Option<FieldActionsPatch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_value: Option<DefaultValueInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_length: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<FieldOptionInput>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub widgets: Option<Vec<WidgetPlacement<C>>>,
}

impl<C: Coordinates> FormFieldDraft<C> {
    /// `draftWritesScripts`: a script among its actions, or a widget action
    /// that holds JavaScript, a submit or a link.
    pub fn writes_scripts(&self) -> bool {
        self.actions.as_ref().is_some_and(FieldActionsPatch::writes_scripts)
            || self
                .widgets
                .iter()
                .flatten()
                .any(|w| w.actions.as_ref().is_some_and(WidgetActionsPatch::writes_scripts))
    }
}

/// What `forms.update` changes (`FormFieldPatch`): the settings it names.
/// `family` is optional (the ref says it); when given it must match.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FormFieldPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family: Option<FormFieldFamily>,
    /// The field's own `/T` segment. Reparenting is not supported.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(flatten)]
    pub flags: FieldFlags,
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub alternate_name: Patch<String>,
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub mapping_name: Patch<String>,
    /// A group is changed, never removed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actions: Option<FieldActionsPatch>,
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub default_value: Patch<DefaultValueInput>,
    /// `Clear` removes the limit; a limit below the current value's length
    /// is refused.
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub max_length: Patch<f64>,
    /// Replace the option list; the selection is re-synced.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<FieldOptionInput>>,
}

impl FormFieldPatch {
    /// A patch as the wire sends it, checked against the field's family
    /// (`assertPatchFitsFamily`) first, on the members it names.
    pub fn of_wire(
        patch: &Map<String, Value>,
        family: FormFieldFamily,
    ) -> Result<Self, EngineError> {
        assert_patch_fits_family(patch, family)?;
        serde_json::from_value(Value::Object(patch.clone())).map_err(|e| {
            EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!("the patch is not the shape a field update takes: {e}"),
            )
        })
    }
}

/// The patch members every family has (`PATCH_BASE_MEMBERS`).
const PATCH_BASE_MEMBERS: [&str; 9] = [
    "family",
    "name",
    "readOnly",
    "required",
    "noExport",
    "alternateName",
    "mappingName",
    "groupId",
    "actions",
];

/// The members each family's patch adds (`PATCH_FAMILY_MEMBERS`).
fn patch_family_members(family: FormFieldFamily) -> &'static [&'static str] {
    match family {
        FormFieldFamily::Text => &["defaultValue", "maxLength", "multiline", "password", "comb"],
        FormFieldFamily::Radio => &["radiosInUnison", "noToggleToOff"],
        FormFieldFamily::Combobox => &["edit", "defaultValue", "options"],
        FormFieldFamily::Listbox => &["multiSelect", "options", "defaultValue"],
        _ => &[],
    }
}

/// `assertPatchFitsFamily`: a patch names no family (the ref says it) or the
/// field's own, and sets only members that family has; a list's default is
/// option values, a text field's or a dropdown's one string.
fn assert_patch_fits_family(
    patch: &Map<String, Value>,
    family: FormFieldFamily,
) -> Result<(), EngineError> {
    let invalid = |message: String, field: &str| {
        EngineError::with_message(EngineErrorCode::InvalidArg, message)
            .with_details(serde_json::json!({ "field": field }))
    };
    if let Some(named) = patch.get("family") {
        let named_family = serde_json::from_value::<FormFieldFamily>(named.clone()).ok();
        if named_family != Some(family) {
            return Err(invalid(
                format!(
                    "patch family '{}' does not match field family '{}'",
                    named.as_str().unwrap_or_default(),
                    family_name(family)
                ),
                "family",
            ));
        }
    }
    let own = patch_family_members(family);
    for member in patch.keys() {
        if !PATCH_BASE_MEMBERS.contains(&member.as_str()) && !own.contains(&member.as_str()) {
            return Err(invalid(
                format!("'{member}' does not apply to a {} field", family_name(family)),
                member,
            ));
        }
    }
    if let Some(default) = patch.get("defaultValue").filter(|value| !value.is_null())
        && default.is_array() != (family == FormFieldFamily::Listbox)
    {
        return Err(invalid(
            if family == FormFieldFamily::Listbox {
                "a list's defaultValue is a list of option values".to_owned()
            } else {
                format!("a {} field's defaultValue is one string", family_name(family))
            },
            "defaultValue",
        ));
    }
    Ok(())
}

/// A family as upstream spells it in a message.
fn family_name(family: FormFieldFamily) -> String {
    serde_json::to_value(family)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_owned())
}

/// A placement once resolved (`PlacedWidget`): where the widget stands, its
/// turn, and its style and actions read into their types.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedWidget {
    pub page: PageObjectNumber,
    pub rect: PdfRect,
    pub rotation: Option<PdfRotation>,
    pub export_value: Option<String>,
    pub actions: Option<WidgetActionsPatch>,
    pub style: WidgetStyle,
}

/// `fitWidgetToFamily`: `/MK /CA` is a push button's caption and a
/// checkbox's or radio button's symbol (`checkStyle`), so each goes on its
/// own family's widgets only: on another widget a `null` (as its row reads)
/// is dropped, and a value is refused.
pub fn fit_widget_to_family(
    family: FormFieldFamily,
    mut value: Map<String, Value>,
) -> Result<Map<String, Value>, EngineError> {
    let toggle = matches!(family, FormFieldFamily::Checkbox | FormFieldFamily::Radio);
    for (key, takes, whose) in [
        ("caption", family == FormFieldFamily::Pushbutton, "a push button's"),
        ("checkStyle", toggle, "a checkbox's or radio button's"),
    ] {
        let Some(given) = value.get(key) else { continue };
        if takes {
            continue;
        }
        if !given.is_null() {
            let widget = if family == FormFieldFamily::Unknown {
                "this widget".to_owned()
            } else {
                format!("a {} field's widget", family_name(family))
            };
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!("'{key}' applies to {whose} widgets, not to {widget}"),
            )
            .with_details(serde_json::json!({ "field": key })));
        }
        value.remove(key);
    }
    Ok(value)
}

/// `placedWidgetOf` (after `fitWidgetToFamily`): a placement as the file
/// holds it, `/Rect` where the widget stands and its turn. A `box` given is
/// turned into the rect it stands in; without one, `rect` is that place.
/// Refused: a placement with neither, and a turn that isn't a quarter turn.
pub fn placed_widget_of(
    family: FormFieldFamily,
    placement: &WidgetPlacement,
) -> Result<PlacedWidget, EngineError> {
    let style_fields = fit_widget_to_family(family, placement.style.clone())?;
    let mut style = WidgetStyle::default();
    for (name, value) in &style_fields {
        if WIDGET_STYLE_NAMES.contains(&name.as_str()) {
            read_style_field("widget placement", name, value, &mut style)?;
        }
    }
    let rotation = match placement.rotation {
        None => None,
        Some(degrees) => {
            if ![0.0, 90.0, 180.0, 270.0].contains(&degrees) {
                return Err(EngineError::with_message(
                    EngineErrorCode::InvalidArg,
                    format!("a widget turns by a quarter turn (0, 90, 180 or 270), not {degrees}"),
                )
                .with_details(serde_json::json!({ "field": "rotation" })));
            }
            crate::features::annotation::write::widget::quarter_turn(degrees).flatten()
        }
    };
    let rect = match (placement.content_box, placement.rect) {
        (Some(content_box), _) => {
            let content_box = normalize_pdf_rect(content_box);
            match rotation {
                Some(turn) => {
                    crate::geom::pdf_rect_turned_bounds(content_box, f64::from(turn.degrees()))
                }
                None => content_box,
            }
        }
        (None, Some(rect)) => rect,
        (None, None) => {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                "a widget's placement needs its `rect` or its `box`",
            )
            .with_details(serde_json::json!({ "field": "rect" })));
        }
    };
    Ok(PlacedWidget {
        page: placement.page,
        rect,
        rotation,
        export_value: placement.export_value.clone(),
        actions: placement.actions.clone(),
        style,
    })
}

/// `onStateOf`: the on-state a new widget of a `family` field gets, its
/// export value. A radio button needs one other than `Off`; a checkbox's is
/// `Yes` when left out; other families take none.
fn on_state_of(family: FormFieldFamily, placement: &PlacedWidget) -> Result<String, EngineError> {
    let invalid = |message: String| {
        EngineError::with_message(EngineErrorCode::InvalidArg, message)
            .with_details(serde_json::json!({ "field": "exportValue" }))
    };
    if !matches!(family, FormFieldFamily::Checkbox | FormFieldFamily::Radio) {
        if placement.export_value.is_some() {
            return Err(invalid(format!(
                "'exportValue' applies to checkbox and radio widgets, not a {} field's",
                family_name(family)
            )));
        }
        return Ok(String::new());
    }
    let value = placement
        .export_value
        .clone()
        .or_else(|| (family == FormFieldFamily::Checkbox).then(|| "Yes".to_owned()))
        .filter(|value| !value.is_empty() && value != "Off");
    value.ok_or_else(|| {
        invalid(if family == FormFieldFamily::Radio {
            "every radio button needs an exportValue other than \"Off\"".to_owned()
        } else {
            "a checkbox's exportValue can't be \"Off\"".to_owned()
        })
    })
}

/// `preflightPlacement`: check a placement before anything is written;
/// answers its page index.
fn preflight_placement(owned: &OwnedDocument, placed: &PlacedWidget) -> Result<u32, EngineError> {
    let page_index = owned.page_index(placed.page)?;
    let PdfRect { left, bottom, right, top } = placed.rect;
    if ![left, bottom, right, top].iter().all(|edge| edge.is_finite()) {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "widget rect must be finite numbers",
        )
        .with_details(serde_json::json!({ "field": "rect" })));
    }
    Ok(page_index)
}

/// `assertChoiceDefault`: a choice field's default is option values, each
/// once, and one unless it's a multi-select list; an editable dropdown's may
/// be free text.
fn assert_choice_default(
    family: FormFieldFamily,
    multi_select: bool,
    edit: bool,
    options: &[FormFieldOption],
    values: &[String],
) -> Result<(), EngineError> {
    let invalid = |message: String| {
        EngineError::with_message(EngineErrorCode::InvalidArg, message)
            .with_details(serde_json::json!({ "field": "defaultValue" }))
    };
    let unique: std::collections::BTreeSet<&String> = values.iter().collect();
    if unique.len() != values.len() {
        return Err(invalid("the default values must not repeat".to_owned()));
    }
    let list = family == FormFieldFamily::Listbox;
    if values.len() > 1 && !(list && multi_select) {
        return Err(invalid(format!(
            "a {} has one default",
            if list { "list without multiSelect" } else { "dropdown" }
        )));
    }
    let free_text = family == FormFieldFamily::Combobox && edit;
    if let Some(unknown) =
        values.iter().find(|value| !options.iter().any(|option| &option.value == *value))
        && !free_text
    {
        return Err(invalid(format!(
            "'{unknown}' is not an option value of the {}",
            if list { "list" } else { "dropdown" }
        )));
    }
    Ok(())
}

fn option_of(input: &FieldOptionInput) -> FormFieldOption {
    FormFieldOption { label: input.label.clone(), value: input.value.clone(), selected: false }
}

/// `values` that are option values, in option order (`inOptionOrder`).
fn in_option_order(options: &[FormFieldOption], values: &[String]) -> Vec<String> {
    super::value::matched_indexes(options, values)
        .into_iter()
        .map(|at| options[at].value.clone())
        .collect()
}

/// `defaultEntryOfDraft`: the default (`/DV`) a draft gives its field: a
/// text field's or dropdown's one value, a list box's option values in
/// option order; none without one.
pub fn default_entry_of_draft<C: Coordinates>(
    draft: &FormFieldDraft<C>,
) -> Result<FormValueEntry, EngineError> {
    let options: Vec<FormFieldOption> = draft.options.iter().flatten().map(option_of).collect();
    let family = draft.family.family();
    if matches!(draft.family, CreatableFamily::Combobox | CreatableFamily::Listbox)
        && let Some(default) = &draft.default_value
    {
        assert_choice_default(
            family,
            draft.flags.multi_select == Some(true),
            draft.flags.edit == Some(true),
            &options,
            &default.values(),
        )?;
    }
    let one = |default: &DefaultValueInput| match default {
        DefaultValueInput::One(value) => Ok(FormValueEntry::Scalar { value: value.clone() }),
        DefaultValueInput::Many(_) => Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!("a {} field's defaultValue is one string", family_name(family)),
        )
        .with_details(serde_json::json!({ "field": "defaultValue" }))),
    };
    match draft.family {
        CreatableFamily::Text | CreatableFamily::Combobox => match &draft.default_value {
            Some(default) => one(default),
            None => Ok(FormValueEntry::None),
        },
        CreatableFamily::Listbox => {
            let values = match &draft.default_value {
                None => Vec::new(),
                Some(DefaultValueInput::Many(values)) => values.clone(),
                Some(DefaultValueInput::One(_)) => {
                    return Err(EngineError::with_message(
                        EngineErrorCode::InvalidArg,
                        "a list's defaultValue is a list of option values",
                    )
                    .with_details(serde_json::json!({ "field": "defaultValue" })));
                }
            };
            Ok(super::value::choice_entry_of(&in_option_order(&options, &values)))
        }
        _ => Ok(FormValueEntry::None),
    }
}

/// `formFieldAfter`, the half a field update writes: the default and the
/// options a patch names, and the selection that goes with new options (the
/// fields a write reads back say the rest). A new name, the flags and the
/// names are written as given.
pub fn form_field_after(
    current: &FormFieldDto,
    patch: &FormFieldPatch,
) -> Result<FormFieldDto, EngineError> {
    let mut field = current.clone();
    if let FormFieldKind::Text { max_length, default_value, .. } = &mut field.kind {
        match &patch.max_length {
            Patch::Keep => {}
            Patch::Clear => *max_length = None,
            Patch::Set(limit) => *max_length = Some(*limit as i32),
        }
        match &patch.default_value {
            Patch::Keep => {}
            Patch::Clear => {
                *default_value = String::new();
                field.base.default_value_entry = FormValueEntry::None;
            }
            Patch::Set(default) => {
                let value = match default {
                    DefaultValueInput::One(value) => value.clone(),
                    DefaultValueInput::Many(_) => {
                        return Err(EngineError::with_message(
                            EngineErrorCode::InvalidArg,
                            "a text field's defaultValue is one string",
                        )
                        .with_details(serde_json::json!({ "field": "defaultValue" })));
                    }
                };
                *default_value = value.clone();
                field.base.default_value_entry = FormValueEntry::Scalar { value };
            }
        }
    }
    if matches!(field.kind, FormFieldKind::Combobox { .. } | FormFieldKind::Listbox { .. }) {
        if let Some(options) = &patch.options {
            field = with_options(&field, options.iter().map(option_of).collect());
        }
        let defaults = match &patch.default_value {
            Patch::Keep => None,
            Patch::Clear => Some(Vec::new()),
            Patch::Set(default) => Some(default.values()),
        };
        if let Some(values) = defaults {
            let (multi_select, edit, options) = match &field.kind {
                FormFieldKind::Listbox { multi_select, options, .. } => {
                    (*multi_select, false, options.clone())
                }
                FormFieldKind::Combobox { edit, options, .. } => (false, *edit, options.clone()),
                _ => (false, false, Vec::new()),
            };
            assert_choice_default(field.base.family, multi_select, edit, &options, &values)?;
            field = with_default(&field, &values, false);
        }
    }
    Ok(field)
}

/// `withOptions`: a choice field with new options: what is still an option
/// stays selected, and stays its default.
fn with_options(field: &FormFieldDto, options: Vec<FormFieldOption>) -> FormFieldDto {
    let exports: Vec<String> = options.iter().map(|option| option.value.clone()).collect();
    let keeps_free_text = matches!(field.kind, FormFieldKind::Combobox { edit: true, .. });
    let current = match &field.kind {
        FormFieldKind::Combobox { value, .. } if value.is_empty() => Vec::new(),
        FormFieldKind::Combobox { value, .. } => vec![value.clone()],
        FormFieldKind::Listbox { selected_values, .. } => selected_values.clone(),
        _ => Vec::new(),
    };
    let kept = kept_values(&current, &exports, keeps_free_text);
    let mut listed = field.clone();
    match &mut listed.kind {
        FormFieldKind::Listbox { options: list, top_index, .. } => {
            // A list box's top row stays among its options.
            *top_index = (*top_index).min(options.len().saturating_sub(1) as u32);
            *list = options;
        }
        FormFieldKind::Combobox { options: list, .. } => *list = options,
        _ => {}
    }
    let reselected = super::value::choice_field_with(&listed, &kept);
    if field.base.default_value_entry == FormValueEntry::None {
        return reselected;
    }
    let defaults = match &field.base.default_value_entry {
        FormValueEntry::Scalar { value } => vec![value.clone()],
        FormValueEntry::Array { values } => values.clone(),
        _ => Vec::new(),
    };
    let kept_defaults = kept_values(&defaults, &exports, keeps_free_text);
    with_default(&reselected, &kept_defaults, true)
}

/// `keptValues`: the values still among `exports`; free text survives when
/// nothing else does.
fn kept_values(values: &[String], exports: &[String], keep_free_text: bool) -> Vec<String> {
    let kept: Vec<String> = values.iter().filter(|v| exports.contains(v)).cloned().collect();
    if kept.is_empty() && keep_free_text && !values.is_empty() {
        return values.to_vec();
    }
    kept
}

/// `withDefault`: a choice field's default set to `values` (option order; a
/// dropdown's free text as it is). `filtered`: what new options left of the
/// old default, where none left is an empty default rather than none.
fn with_default(field: &FormFieldDto, values: &[String], filtered: bool) -> FormFieldDto {
    let mut after = field.clone();
    let options = match &field.kind {
        FormFieldKind::Combobox { options, .. } | FormFieldKind::Listbox { options, .. } => {
            options.clone()
        }
        _ => return after,
    };
    let multi_select = matches!(field.kind, FormFieldKind::Listbox { multi_select: true, .. });
    let (entry, ordered) = if values.is_empty() {
        let entry = if !filtered {
            FormValueEntry::None
        } else if multi_select {
            FormValueEntry::Array { values: Vec::new() }
        } else {
            FormValueEntry::Scalar { value: String::new() }
        };
        (entry, Vec::new())
    } else if super::value::choice_matches(&options, values) {
        let ordered = in_option_order(&options, values);
        (super::value::choice_entry_of(&ordered), ordered)
    } else {
        (FormValueEntry::Scalar { value: values[0].clone() }, values.to_vec())
    };
    after.base.default_value_entry = entry;
    match &mut after.kind {
        FormFieldKind::Combobox { default_value, .. } => {
            *default_value = ordered.first().cloned().unwrap_or_default();
        }
        FormFieldKind::Listbox { default_value, .. } => *default_value = ordered,
        _ => {}
    }
    after
}

/// `writeFieldEntry(…, 'default')`: `/DV` as `entry` holds it.
fn write_default(
    document: &Document,
    object_number: u32,
    before: &FormValueEntry,
    after: &FormValueEntry,
) -> Result<(), EngineError> {
    if super::value::value_entries_equal(before, after) {
        return Ok(());
    }
    let write = match after {
        FormValueEntry::None => pdfium::FieldValueWrite::None,
        FormValueEntry::Scalar { value } => pdfium::FieldValueWrite::Scalar(value),
        FormValueEntry::Array { values } => pdfium::FieldValueWrite::Array(values),
        FormValueEntry::Unsupported => return Err(rejected("default value rejected")),
    };
    if !document.set_field_default_value(object_number, write) {
        return Err(rejected("default value rejected"));
    }
    Ok(())
}

fn rejected(message: &str) -> EngineError {
    EngineError::with_message(EngineErrorCode::InvalidArg, message)
}

fn write_options(
    document: &Document,
    object_number: u32,
    options: &[FieldOptionInput],
) -> Result<(), EngineError> {
    let labels: Vec<String> = options.iter().map(|o| o.label.clone()).collect();
    let exports: Vec<String> = options.iter().map(|o| o.value.clone()).collect();
    if !document.set_field_options(object_number, &labels, &exports) {
        return Err(rejected("options rejected"));
    }
    Ok(())
}

/// Who writes a structural field write, and with what: the document and
/// its pages, the value writer (for its checks and its meta stamp), and
/// what a new widget is born with.
pub struct FieldWriter<'a> {
    pub owned: &'a OwnedDocument,
    pub mutator: FormMutator<'a>,
    pub context: AnnotationWriteContext<'a>,
    /// A new widget's `/NM`, a fresh UUIDv7.
    pub new_name: Box<dyn Fn() -> String + 'a>,
    /// The creation stamp's date, as a PDF date.
    pub now: Option<String>,
}

/// A create checked, ready to write (`createField` up to the apply
/// boundary): each widget placed, its page and on-state known, the default
/// stated.
#[derive(Debug, Clone)]
pub struct PlannedCreate {
    placements: Vec<(PlacedWidget, u32, String)>,
    default_entry: FormValueEntry,
}

/// `createField` up to the apply boundary: everything a draft can be wrong
/// about, checked before anything is written.
pub fn plan_create(
    owned: &OwnedDocument,
    draft: &FormFieldDraft,
    widget_object_numbers: Option<&[u32]>,
) -> Result<PlannedCreate, EngineError> {
    let family = draft.family.family();
    let mut placements = Vec::new();
    let mut placed_all = Vec::new();
    for placement in draft.widgets.iter().flatten() {
        placed_all.push(placed_widget_of(family, placement)?);
    }
    let mut pages = Vec::new();
    for placed in &placed_all {
        pages.push(preflight_placement(owned, placed)?);
    }
    let mut states = Vec::new();
    for placed in &placed_all {
        states.push(on_state_of(family, placed)?);
    }
    for ((placed, page), state) in placed_all.into_iter().zip(pages).zip(states) {
        placements.push((placed, page, state));
    }
    // The default is checked against the draft before anything is written.
    let default_entry = default_entry_of_draft(draft)?;
    if let Some(limit) = draft.max_length
        && (limit.fract() != 0.0 || limit <= 0.0 || !limit.is_finite())
    {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!("maxLength must be a positive integer, got {limit}"),
        )
        .with_details(serde_json::json!({ "field": "maxLength" })));
    }
    if let Some(numbers) = widget_object_numbers
        && numbers.len() != placements.len()
    {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!(
                "forms.create needs one widget object number per widget: {} for {}",
                numbers.len(),
                placements.len()
            ),
        )
        .with_details(serde_json::json!({ "field": "widgetObjectNumbers" })));
    }
    Ok(PlannedCreate { placements, default_entry })
}

/// What a create or an update did: the field read back, and the
/// calculation order before and after, when the write changed it.
#[derive(Debug, Clone)]
pub struct WrittenField {
    pub field: FormFieldDto,
    pub calculation_order: Option<OrderChange>,
}

/// `createField` from the apply boundary: the field at `object_number` (the
/// next free one when absent), its settings, who created it and its group,
/// its widgets at `widget_object_numbers`, and its place in the calculation
/// order when it has a calculate script.
pub fn create_field(
    writer: &FieldWriter<'_>,
    draft: &FormFieldDraft,
    plan: &PlannedCreate,
    object_number: Option<u32>,
    widget_object_numbers: Option<&[u32]>,
    actor: &IdentityClaims,
    cancel: &CancelToken,
) -> Result<WrittenField, EngineError> {
    cancel.check()?;
    let document = writer.owned.document();
    let (field_object_number, calculation_order) = in_layer_transaction(document, || {
        let created = document
            .create_field(draft.family.native(), &draft.name, object_number.unwrap_or(0))
            .ok_or_else(|| {
                rejected(&format!(
                    "cannot create field \"{}\" (name conflict or invalid)",
                    draft.name
                ))
            })?;
        configure_new_field(document, draft, &plan.default_entry, created)?;
        stamp_field_creation(document, created, actor, writer.now.as_deref())?;
        if let Some(group) = &draft.group_id {
            write_field_group(document, created, group)?;
        }
        for (at, (placed, page_index, on_state)) in plan.placements.iter().enumerate() {
            let number = widget_object_numbers.and_then(|numbers| numbers.get(at).copied());
            let widget = create_unattached_widget(writer, *page_index, placed, number)?;
            // A new field is never merged, so no split number is needed.
            let adopted = document
                .attach_widget(created, widget, on_state, 0)
                .map_err(|_| rejected("the export value is not a name a PDF can hold"))?;
            if !adopted {
                return Err(EngineError::with_message(
                    EngineErrorCode::Unknown,
                    "widget adoption failed",
                ));
            }
        }
        let order = follow_calculate_script(
            document,
            created,
            draft.actions.as_ref().map(|actions| &actions.calculate),
        )?;
        Ok((created, order))
    })?;
    let field = read_back_field(document, field_object_number, cancel)?;
    Ok(WrittenField { field, calculation_order })
}

/// `configureNewField`: the draft's field-plane settings, on a field just
/// created.
fn configure_new_field(
    document: &Document,
    draft: &FormFieldDraft,
    default_entry: &FormValueEntry,
    object_number: u32,
) -> Result<(), EngineError> {
    let (set, clear) = draft.flags.masks();
    if (set, clear) != (0, 0) {
        // Upstream leaves this call's answer unread.
        let _ = document.set_field_flags(object_number, set, clear);
    }
    if let Some(options) = &draft.options {
        write_options(document, object_number, options)?;
    }
    write_default(document, object_number, &FormValueEntry::None, default_entry)?;
    if let Some(limit) = draft.max_length
        && !document.set_field_max_length(object_number, limit as i32)
    {
        return Err(rejected("maxLength rejected"));
    }
    if let Some(name) = &draft.alternate_name
        && !document.set_field_alternate_name(object_number, name)
    {
        return Err(rejected("alternate name rejected"));
    }
    if let Some(name) = &draft.mapping_name
        && !document.set_field_mapping_name(object_number, name)
    {
        return Err(rejected("mapping name rejected"));
    }
    if let Some(actions) = &draft.actions {
        write_field_scripts(document, object_number, None, actions)?;
    }
    Ok(())
}

/// `/EMBD_Metadata` keys of a field's creation and group
/// (`fieldAttribution.ts`).
const KEY_SCHEMA_VERSION: &str = "SchemaVersion";
const KEY_GROUP_ID: &str = "GroupID";
const KEY_CREATED_BY: &str = "CreatedBy";
const KEY_CREATED_AT: &str = "CreatedAt";
const KEY_IMPORTED_BY: &str = "ImportedBy";

/// `writeAttribution`: each entry written, the ones with no value cleared.
fn write_attribution(
    document: &Document,
    object_number: u32,
    entries: &[(&str, Option<&str>)],
) -> Result<(), EngineError> {
    let failed = || {
        EngineError::with_message(
            EngineErrorCode::Unknown,
            format!("the metadata of field {object_number} could not be written"),
        )
    };
    let wrote = document
        .set_field_embed_metadata_number(
            object_number,
            KEY_SCHEMA_VERSION,
            crate::features::annotation::write::primitives::EMBD_METADATA_SCHEMA_VERSION,
        )
        .map_err(|_| failed())?;
    if !wrote {
        return Err(failed());
    }
    for (key, value) in entries {
        let wrote = match value {
            Some(value) => document.set_field_embed_metadata_string(object_number, key, value),
            None => document.clear_field_embed_metadata_key(object_number, key),
        };
        if !wrote.map_err(|_| failed())? {
            return Err(failed());
        }
    }
    Ok(())
}

/// `stampFieldCreation`: who created a field, and when. An anonymous session
/// stamps nothing.
fn stamp_field_creation(
    document: &Document,
    object_number: u32,
    actor: &IdentityClaims,
    now: Option<&str>,
) -> Result<(), EngineError> {
    let Some(user_id) = actor.user_id.as_deref().filter(|id| !id.is_empty()) else {
        return Ok(());
    };
    write_attribution(
        document,
        object_number,
        &[(KEY_CREATED_BY, Some(user_id)), (KEY_CREATED_AT, now)],
    )
}

/// `restoreFieldCreation`: who created a field and when, as a restoring
/// import has it (`created_at` ISO 8601), and the importing session's user
/// as `importedBy`. An absent one clears the entry.
pub fn restore_field_creation(
    document: &Document,
    object_number: u32,
    created_by: Option<&str>,
    created_at: Option<&str>,
    imported_by: Option<&str>,
) -> Result<(), EngineError> {
    let created_at = created_at.map(crate::clock::format_pdf_date).transpose()?;
    write_attribution(
        document,
        object_number,
        &[
            (KEY_CREATED_BY, created_by),
            (KEY_CREATED_AT, created_at.as_deref()),
            (KEY_IMPORTED_BY, imported_by),
        ],
    )
}

/// `restoreFieldFill`: who filled a field in and when, as a restoring import
/// has it, and the importing session's user as `importedBy`. An absent one
/// clears the entry.
pub fn restore_field_fill(
    document: &Document,
    object_number: u32,
    filled_by: Option<&str>,
    filled_by_name: Option<&str>,
    filled_at: Option<&str>,
    imported_by: Option<&str>,
) -> Result<(), EngineError> {
    let filled_at = filled_at.map(crate::clock::format_pdf_date).transpose()?;
    write_attribution(
        document,
        object_number,
        &[
            ("FilledBy", filled_by),
            ("FilledByName", filled_by_name),
            ("FilledAt", filled_at.as_deref()),
            (KEY_IMPORTED_BY, imported_by),
        ],
    )
}

/// `writeFieldGroup`: put a field in `group_id`. A group is changed, never
/// removed, so there is no clearing.
pub fn write_field_group(
    document: &Document,
    object_number: u32,
    group_id: &str,
) -> Result<(), EngineError> {
    write_attribution(document, object_number, &[(KEY_GROUP_ID, Some(group_id))])
}

/// `createUnattachedWidget`: a widget born through the annotation plane at
/// `object_number` (the next free one when absent), with a fresh `/NM`,
/// placed and turned, styled with the placement's style fields, and given
/// its actions. Answers its object number, ready for adoption, which draws
/// its appearance turned.
fn create_unattached_widget(
    writer: &FieldWriter<'_>,
    page_index: u32,
    placed: &PlacedWidget,
    object_number: Option<u32>,
) -> Result<u32, EngineError> {
    use crate::features::annotation::subtype::AnnotationSubtype;
    let document = writer.owned.document();
    let annotation = document
        .create_annotation_raw(
            page_index,
            AnnotationSubtype::Widget.code(),
            object_number.unwrap_or(0),
        )
        .ok_or_else(|| match object_number {
            Some(number) => crate::session::document::object_number_unavailable(number, "taken"),
            None => EngineError::with_message(
                EngineErrorCode::Unknown,
                "failed to create widget annotation",
            ),
        })?;
    crate::features::annotation::write::primitives::set_string(
        &annotation,
        "NM",
        &(writer.new_name)(),
    )?;
    crate::features::annotation::write::primitives::set_rect(&annotation, placed.rect)?;
    if let Some(turn) = placed.rotation {
        crate::features::annotation::write::widget::set_widget_turn(&annotation, Some(turn))?;
    }
    if placed.style != WidgetStyle::default() {
        apply_widget_style(&annotation, &placed.style, &writer.context)?;
    }
    if let Some(actions) = &placed.actions {
        write_widget_actions(document, &annotation, Some(actions))?;
    }
    let widget_object_number = annotation.object_number();
    if widget_object_number == 0 {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "widget annotation has no object number",
        ));
    }
    Ok(widget_object_number)
}

/// `readBackField`: the field read through a fresh model.
fn read_back_field(
    document: &Document,
    object_number: u32,
    cancel: &CancelToken,
) -> Result<FormFieldDto, EngineError> {
    read_field_back(document, object_number, cancel).map_err(|e| {
        if e.code == EngineErrorCode::Unknown {
            EngineError::with_message(EngineErrorCode::Unknown, "form field vanished after write")
        } else {
            e
        }
    })
}

/// A field as a write found it: its object number and its read.
pub struct FoundField {
    pub object_number: u32,
    pub field: FormFieldDto,
}

/// Resolve `reference`, check it can be written (`assertWritable`), and read
/// the field.
fn found_writable(
    mutator: &FormMutator<'_>,
    reference: &FormFieldRef,
) -> Result<FoundField, EngineError> {
    let model = load_model(mutator.document)?;
    let resolved = resolve(&model, reference)?;
    mutator.assert_writable(&model, &resolved)?;
    let field = read_at(&model, resolved.index)?;
    Ok(FoundField { object_number: resolved.object_number, field })
}

/// `updateField`: the settings a patch names, each written only as the
/// field's rules leave them, and the calculation order kept in step with a
/// calculate script.
pub fn update_field(
    mutator: &FormMutator<'_>,
    reference: &FormFieldRef,
    patch: &FormFieldPatch,
    cancel: &CancelToken,
) -> Result<WrittenField, EngineError> {
    cancel.check()?;
    let document = mutator.document;
    let FoundField { object_number, field: before } = found_writable(mutator, reference)?;
    // Checked against the field as the patch leaves it, before anything is
    // written.
    let after = form_field_after(&before, patch)?;
    cancel.check()?;

    let calculation_order = in_layer_transaction(document, || {
        if let Some(name) = &patch.name
            && !document.set_field_name(object_number, name)
        {
            return Err(rejected(&format!(
                "cannot rename to \"{name}\" (sibling conflict or invalid)"
            )));
        }
        let (set, clear) = patch.flags.masks();
        if (set, clear) != (0, 0) && !document.set_field_flags(object_number, set, clear) {
            return Err(rejected("flag update rejected"));
        }
        let limit = match patch.max_length {
            Patch::Keep => None,
            Patch::Clear => Some(0),
            Patch::Set(limit) => Some(limit as i32),
        };
        if let Some(limit) = limit
            && !document.set_field_max_length(object_number, limit)
        {
            return Err(rejected("maxLength rejected (current value exceeds it)"));
        }
        for (value, alternate) in [(&patch.alternate_name, true), (&patch.mapping_name, false)] {
            let text = match value {
                Patch::Keep => continue,
                Patch::Clear => "",
                Patch::Set(text) => text.as_str(),
            };
            let ok = if alternate {
                document.set_field_alternate_name(object_number, text)
            } else {
                document.set_field_mapping_name(object_number, text)
            };
            if !ok {
                return Err(rejected(if alternate {
                    "alternate name rejected"
                } else {
                    "mapping name rejected"
                }));
            }
        }
        if let Some(group) = &patch.group_id
            && Some(group) != before.base.group_id.as_ref()
        {
            write_field_group(document, object_number, group)?;
        }
        // New options and a new default, and the selection that goes with
        // them, as the field's rules work them out.
        let new_options = patch.options.is_some();
        let new_default = !patch.default_value.is_keep();
        if new_options || new_default {
            if let Some(options) = &patch.options {
                write_options(document, object_number, options)?;
            }
            write_default(
                document,
                object_number,
                &before.base.default_value_entry,
                &after.base.default_value_entry,
            )?;
            if new_options {
                mutator.write_field_after(object_number, &before, &after, true)?;
            }
        }
        if let Some(actions) = &patch.actions {
            write_field_scripts(document, object_number, before.base.actions.as_ref(), actions)?;
        }
        follow_calculate_script(
            document,
            object_number,
            patch.actions.as_ref().map(|actions| &actions.calculate),
        )
    })?;
    let field = read_back_field(document, object_number, cancel)?;
    Ok(WrittenField { field, calculation_order })
}

/// `followCalculateScript`: keep the calculation order in step with a
/// field's calculate script, as a write left it: a script puts the field at
/// the end when it isn't in the order, removing the script takes it out.
/// Answers the order before and after, when it changed.
fn follow_calculate_script(
    document: &Document,
    object_number: u32,
    calculate: Option<&Patch<crate::features::actions::write::FieldScriptWrite>>,
) -> Result<Option<OrderChange>, EngineError> {
    let Some(calculate) = calculate.filter(|patch| !patch.is_keep()) else {
        return Ok(None);
    };
    let before = read_calculation_order(&load_model(document)?);
    let after: Vec<u32> = if calculate.is_clear() {
        before.iter().copied().filter(|n| *n != object_number).collect()
    } else if before.contains(&object_number) {
        before.clone()
    } else {
        before.iter().copied().chain(std::iter::once(object_number)).collect()
    };
    if same_order(&before, &after) {
        return Ok(None);
    }
    write_calculation_order(document, &after)?;
    Ok(Some(OrderChange { before, after }))
}

/// Where fields go in the calculation order (`FieldPosition`).
pub type FieldPosition = crate::mutation::list_position::ListPosition<FormFieldRef>;

/// `reorderCalculations`: the fields go together, in the order given, to
/// `position`. Every field named must be in the order. Answers the order
/// before and after, and the fields that moved.
pub fn reorder_calculations(
    document: &Document,
    fields: &[FormFieldRef],
    position: &FieldPosition,
    cancel: &CancelToken,
) -> Result<(OrderChange, Vec<u32>), EngineError> {
    use crate::mutation::list_position::{ListPosition, reordered_list};
    cancel.check()?;
    let model = load_model(document)?;
    let number_of = |reference: &FormFieldRef| resolve(&model, reference).map(|r| r.object_number);
    let before = read_calculation_order(&model);
    let moved = fields.iter().map(number_of).collect::<Result<Vec<u32>, _>>()?;
    let at: ListPosition<u32> = match position {
        ListPosition::End(end) => ListPosition::End(*end),
        ListPosition::Before { before } => ListPosition::Before { before: number_of(before)? },
        ListPosition::After { after } => ListPosition::After { after: number_of(after)? },
    };
    drop(model);
    let after = reordered_list(&before, &moved, &at, |n| *n)?;
    cancel.check()?;
    if !same_order(&before, &after) {
        write_calculation_order(document, &after)?;
    }
    Ok((OrderChange { before, after }, moved))
}

/// What a field delete removed (`deleteField`'s answer).
#[derive(Debug, Clone)]
pub struct DeletedField {
    pub deleted: FormFieldRef,
    pub removed_widgets: Vec<FormWidgetRef>,
    pub calculation_order: Option<OrderChange>,
}

/// `deleteField`: a terminal field and its widgets in one write. The native
/// write detaches kid widgets and unlinks the field from `/Fields` or its
/// parent's `/Kids`; the cascade then removes every placed widget from its
/// page. A merged field/widget has no kid to detach: its own dictionary
/// leaves its page in the same cascade.
pub fn delete_field(
    owned: &OwnedDocument,
    mutator: &FormMutator<'_>,
    reference: &FormFieldRef,
    cancel: &CancelToken,
) -> Result<DeletedField, EngineError> {
    cancel.check()?;
    let document = owned.document();
    let FoundField { object_number, field: before } = found_writable(mutator, reference)?;
    let removed_widgets: Vec<FormWidgetRef> = before.widgets().copied().collect();
    // Apply boundary: the native delete validates before it writes, so a
    // refusal leaves the document untouched.
    cancel.check()?;
    let calculation_order = in_layer_transaction(document, || {
        // A deleted field's calculate script runs no more: it leaves the
        // order first, while the order can still name it.
        let order = follow_calculate_script(document, object_number, Some(&Patch::Clear))?;
        if document.delete_field(object_number).is_none() {
            return Err(rejected("field cannot be deleted"));
        }
        for widget in &removed_widgets {
            // Direct or unplaced: no `/Annots` entry to remove.
            let Ok(page) = PageObjectNumber::new(widget.page_object_number) else { continue };
            if widget.annot_object_number == 0 {
                continue;
            }
            let reference = AnnotationRef::ObjectNumber { page, annot: widget.annot_object_number };
            crate::features::annotation::write::delete::delete_released_widget(
                document,
                owned.page_index(page)?,
                &reference,
                object_number,
            )?;
        }
        Ok(order)
    })?;
    Ok(DeletedField { deleted: before.base.field_ref.clone(), removed_widgets, calculation_order })
}

/// An `addWidget` checked, ready to write.
#[derive(Debug, Clone)]
pub struct PlannedWidget {
    object_number: u32,
    placed: PlacedWidget,
    page_index: u32,
    on_state: String,
    /// The page of the field's own widget half, when the field is merged.
    merged_page: Option<PageObjectNumber>,
}

impl PlannedWidget {
    /// Whether adding the widget splits a merged field (and so uses a split
    /// number).
    pub fn splits(&self) -> bool {
        self.merged_page.is_some()
    }

    /// The field it goes in.
    pub fn field_object_number(&self) -> u32 {
        self.object_number
    }
}

/// `addWidget` up to the apply boundary.
pub fn plan_add_widget(
    owned: &OwnedDocument,
    mutator: &FormMutator<'_>,
    reference: &FormFieldRef,
    placement: &WidgetPlacement,
) -> Result<PlannedWidget, EngineError> {
    let FoundField { object_number, field: before } = found_writable(mutator, reference)?;
    let placed = placed_widget_of(before.base.family, placement)?;
    let page_index = preflight_placement(owned, &placed)?;
    let on_state = on_state_of(before.base.family, &placed)?;
    // A merged field/widget splits: its widget half becomes a new widget, at
    // a new number in the same place in that page's `/Annots`.
    let merged_page = before
        .widgets()
        .find(|widget| widget.annot_object_number == object_number)
        .and_then(|widget| PageObjectNumber::new(widget.page_object_number).ok());
    Ok(PlannedWidget { object_number, placed, page_index, on_state, merged_page })
}

/// `addWidget` from the apply boundary: the widget where the placement
/// says, styled by it, added to the field, as one write. Answers the field
/// read back and the new widget.
pub fn add_widget(
    writer: &FieldWriter<'_>,
    plan: &PlannedWidget,
    object_number: Option<u32>,
    split_object_number: Option<u32>,
    cancel: &CancelToken,
) -> Result<(FormFieldDto, FormWidgetRef), EngineError> {
    cancel.check()?;
    let document = writer.owned.document();
    let widget = in_layer_transaction(document, || {
        if let Some(page) = plan.merged_page {
            crate::features::annotation::write::delete::promote_inline_annotations(
                document,
                writer.owned.page_index(page)?,
                page,
            )?;
        }
        let widget =
            create_unattached_widget(writer, plan.page_index, &plan.placed, object_number)?;
        let split = if plan.splits() { split_object_number.unwrap_or(0) } else { 0 };
        let adopted = document
            .attach_widget(plan.object_number, widget, &plan.on_state, split)
            .map_err(|_| rejected("the export value is not a name a PDF can hold"))?;
        if !adopted {
            return Err(rejected("the widget could not join the field"));
        }
        Ok(widget)
    })?;
    let field = read_back_field(document, plan.object_number, cancel)?;
    Ok((
        field,
        FormWidgetRef { annot_object_number: widget, page_object_number: plan.placed.page.get() },
    ))
}

/// `detachWidget`: the widget leaves its field and stays on its page, an
/// inert annotation. A merged field/widget is refused: the widget is the
/// field's own dictionary. Answers the field read back and the widget.
pub fn detach_widget(
    mutator: &FormMutator<'_>,
    reference: &FormFieldRef,
    widget: &AnnotationRef,
    cancel: &CancelToken,
) -> Result<(FormFieldDto, FormWidgetRef), EngineError> {
    cancel.check()?;
    let document = mutator.document;
    let model = load_model(document)?;
    let resolved = resolve(&model, reference)?;
    mutator.assert_writable(&model, &resolved)?;
    drop(model);
    let AnnotationRef::ObjectNumber { annot: widget_number, page } = widget else {
        return Err(rejected("widget must be addressed by object number"));
    };
    if *widget_number == resolved.object_number {
        return Err(rejected(&format!(
            "widget {widget_number} is the field's own dictionary (a merged field/widget) and \
             cannot be removed from it - delete the field with doc.forms.delete"
        )));
    }
    if !document.detach_widget(resolved.object_number, *widget_number) {
        return Err(rejected("widget is not attached to this field"));
    }
    let field = read_back_field(document, resolved.object_number, cancel)?;
    Ok((
        field,
        FormWidgetRef { annot_object_number: *widget_number, page_object_number: page.get() },
    ))
}

/// The field a widget dictionary belongs to, by object number, as the form
/// model joins it; `None` for a widget in no field (or not a widget).
pub fn field_of_widget(document: &Document, widget_object_number: u32) -> Option<u32> {
    let model = document.form_model()?;
    let index = model.field_index_for_widget(widget_object_number)?;
    model.field(index).map(|field| field.object_number()).filter(|n| *n > 0)
}

/// The calculation order as the form reads it now.
pub fn calculation_order_now(document: &Document) -> Result<Vec<u32>, EngineError> {
    Ok(read_calculation_order(&load_model(document)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn a_patch_names_only_what_its_family_has() {
        let fits = |patch: Value, family| assert_patch_fits_family(&map(patch), family);
        fits(serde_json::json!({ "name": "x", "multiline": true }), FormFieldFamily::Text).unwrap();
        let refused =
            fits(serde_json::json!({ "multiline": true }), FormFieldFamily::Radio).unwrap_err();
        assert_eq!(refused.message, "'multiline' does not apply to a radio field");
        let wrong =
            fits(serde_json::json!({ "family": "text" }), FormFieldFamily::Listbox).unwrap_err();
        assert_eq!(wrong.message, "patch family 'text' does not match field family 'listbox'");
        let list =
            fits(serde_json::json!({ "defaultValue": "a" }), FormFieldFamily::Listbox).unwrap_err();
        assert_eq!(list.message, "a list's defaultValue is a list of option values");
        let flags = fits(serde_json::json!({ "flags": {} }), FormFieldFamily::Listbox).unwrap_err();
        assert_eq!(flags.details, Some(serde_json::json!({ "field": "flags" })));
    }

    #[test]
    fn a_caption_goes_on_a_push_button_and_a_symbol_on_a_toggle() {
        let style = map(serde_json::json!({ "caption": null, "checkStyle": "star" }));
        let fitted = fit_widget_to_family(FormFieldFamily::Checkbox, style.clone()).unwrap();
        assert_eq!(fitted, map(serde_json::json!({ "checkStyle": "star" })));
        let refused = fit_widget_to_family(FormFieldFamily::Text, style).unwrap_err();
        assert_eq!(refused.details, Some(serde_json::json!({ "field": "checkStyle" })));
    }

    #[test]
    fn a_toggle_widget_turns_on_with_its_export_value() {
        let placed = |export_value: Option<&str>| PlacedWidget {
            page: PageObjectNumber::new(3).unwrap(),
            rect: PdfRect { left: 0.0, bottom: 0.0, right: 10.0, top: 10.0 },
            rotation: None,
            export_value: export_value.map(str::to_owned),
            actions: None,
            style: WidgetStyle::default(),
        };
        assert_eq!(on_state_of(FormFieldFamily::Checkbox, &placed(None)).unwrap(), "Yes");
        assert_eq!(on_state_of(FormFieldFamily::Radio, &placed(Some("a"))).unwrap(), "a");
        assert!(on_state_of(FormFieldFamily::Radio, &placed(None)).is_err());
        assert!(on_state_of(FormFieldFamily::Checkbox, &placed(Some("Off"))).is_err());
        assert!(on_state_of(FormFieldFamily::Text, &placed(Some("a"))).is_err());
        assert_eq!(on_state_of(FormFieldFamily::Text, &placed(None)).unwrap(), "");
    }
}
