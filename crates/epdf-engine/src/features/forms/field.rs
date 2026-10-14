//! Creating, changing and removing a form field.
//!
//! G12. Ports the structural half of
//! `services/src/features/forms/FormMutator.ts` and
//! `internal/{authorWidget,fieldFlagBits}.ts`.
//!
//! # A field and its widgets are born apart and joined
//!
//! `EPDFForm_CreateField` makes the FIELD: a dictionary in `/AcroForm
//! /Fields` with a name, a family and no geometry. What a reader sees on a
//! page is a WIDGET, which is an annotation, and it is born through the
//! annotation plane exactly as any other annotation is. Adoption is a third
//! call that ties them together.
//!
//! Composing the three is what `create_field` does, and the composition is
//! not a transaction. A failure halfway leaves a field with fewer widgets
//! than were asked for, which is a partially configured form rather than an
//! inconsistent one: every object that exists is well formed and reachable.
//! Unwinding would mean deleting objects this layer cannot prove nothing
//! else now points at.
//!
//! # The family-defining bits are not patchable
//!
//! `/Ff` bit 16 makes a radio, bit 17 a push button and bit 18 a combo box.
//! `EPDFForm_CreateField` sets them and the setter refuses them afterwards,
//! because changing one would turn a field into a different kind of control
//! while its widgets, its value and its options stayed as they were. So
//! [`FieldFlags`] does not name them.

use serde::{Deserialize, Serialize};

use pdfium::{CreatableFieldFamily, Document};

use super::snapshot::{FormFieldDto, FormFieldFamily, FormFieldRef, FormWidgetRef};
use super::write::{load_model, read_field_back, require_unlocked, require_writable, resolve};
use crate::cancel::CancelToken;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::actions::vocabulary::ActionReadBudgetTracker;
use crate::geom::PdfRect;
use crate::identity::PageObjectNumber;

/// The `/Ff` bits a draft or a patch may set.
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
    /// The bits to set and the bits to clear.
    ///
    /// The shifts are 0-based and the spec's bit numbers are 1-based, which
    /// is where a table read straight off §12.7.4 goes wrong by one. They
    /// are written as shifts so the numbers here are the numbers the fixture
    /// generator uses.
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

/// One choice of a combo box or a list box.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldOptionInput {
    /// What a reader shows.
    pub label: String,
    /// What a value write and an export use.
    pub value: String,
}

/// Where and how a widget is born.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetPlacement {
    pub page_object_number: PageObjectNumber,
    pub rect: PdfRect,
    /// The appearance state this widget turns on with. Required on every
    /// radio widget, because the group addresses them by it; defaults to
    /// `Yes` for a checkbox; meaningless for the rest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_state: Option<String>,
    /// The widget-plane styling, which is the annotation kind's own draft.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearance: Option<crate::features::annotation::write::draft::WidgetStyleFields>,
}

/// What a caller creates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormFieldDraft {
    /// Which control to make. Push buttons and signatures are not on this
    /// list: the fork's creator does not make them.
    pub family: CreatableFamily,
    /// The dotted fully qualified name. Missing ancestors are created and a
    /// sibling collision is refused.
    pub name: String,
    /// The `/Ff` bits, which ride at the TOP level of the wire object
    /// rather than under a key of their own. That is the TypeScript's
    /// shape, and it is the shape a caller writing `{ multiline: true }`
    /// beside `maxLength` expects.
    #[serde(flatten)]
    pub flags: FieldFlags,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alternate_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mapping_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_length: Option<i32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<FieldOptionInput>,
    /// Where its widgets go. A radio group takes several and everything
    /// else takes at most one, which is why the wire has TWO keys: `widget`
    /// for the families with one and `widgets` for the group. Both land
    /// here, because the difference is in what a family can hold rather
    /// than in what this writer does with them.
    ///
    /// A field with none is a field with no appearance anywhere. Legal, and
    /// what an import-only field looks like.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub widgets: Vec<WidgetPlacement>,
    /// The singular spelling. Folded into `widgets` by
    /// [`FormFieldDraft::placements`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub widget: Option<WidgetPlacement>,
}

impl FormFieldDraft {
    /// Every placement, whichever key it arrived under.
    ///
    /// A draft that used both is not refused: the two keys are one concept,
    /// and a caller that filled in both meant all of them.
    pub fn placements(&self) -> impl Iterator<Item = &WidgetPlacement> {
        self.widget.iter().chain(self.widgets.iter())
    }
}

/// The families a caller may create.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CreatableFamily {
    Text,
    Checkbox,
    Radio,
    Combobox,
    Listbox,
}

impl CreatableFamily {
    fn native(self) -> CreatableFieldFamily {
        match self {
            Self::Text => CreatableFieldFamily::Text,
            Self::Checkbox => CreatableFieldFamily::Checkbox,
            Self::Radio => CreatableFieldFamily::Radio,
            Self::Combobox => CreatableFieldFamily::Combobox,
            Self::Listbox => CreatableFieldFamily::Listbox,
        }
    }

    /// What a widget of this family turns on with when the caller says
    /// nothing. A radio has no default: its widgets are told apart by their
    /// states, so one without is unaddressable and is refused.
    fn default_on_state(self) -> Option<&'static str> {
        match self {
            Self::Checkbox => Some("Yes"),
            Self::Radio => None,
            _ => Some(""),
        }
    }
}

/// What a caller changes.
///
/// `family` is repeated so a patch aimed at the wrong field is refused
/// rather than half-applied: the members that follow are family-specific,
/// and a text patch on a list box would write a `/MaxLen` onto a control
/// that has no text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormFieldPatch {
    pub family: FormFieldFamily,
    /// Rename the field's own `/T` segment. Reparenting is not supported:
    /// a dotted path here would be a name containing dots rather than a
    /// move.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(flatten)]
    pub flags: FieldFlags,
    #[serde(default, skip_serializing_if = "crate::dto::Patch::is_keep")]
    pub alternate_name: crate::dto::Patch<String>,
    #[serde(default, skip_serializing_if = "crate::dto::Patch::is_keep")]
    pub mapping_name: crate::dto::Patch<String>,
    #[serde(default, skip_serializing_if = "crate::dto::Patch::is_keep")]
    pub default_value: crate::dto::Patch<String>,
    /// `Clear` removes the limit. A limit below the current value's length
    /// is refused rather than truncating what is already there.
    #[serde(default, skip_serializing_if = "crate::dto::Patch::is_keep")]
    pub max_length: crate::dto::Patch<i32>,
    /// Replace the option list entirely. The selection is re-synced by
    /// PDFium: an export value that is no longer an option is dropped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<FieldOptionInput>>,
}

/// What a deletion removed and what it left behind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormFieldDeleteResult {
    pub deleted_field_object_number: u32,
    /// The widgets that are now inert annotations. Removing them from their
    /// pages is the annotation plane's job, and this is how a caller knows
    /// which to ask about.
    pub detached_widgets: Vec<FormWidgetRef>,
}

/// Make a field, its widgets, and the joins between them.
pub fn create_field(
    document: &Document,
    page: &dyn WidgetPageSource,
    draft: &FormFieldDraft,
    cancel: &CancelToken,
) -> Result<FormFieldDto, EngineError> {
    cancel.check()?;
    preflight_draft(draft)?;

    let field_object_number =
        document.create_field(draft.family.native(), &draft.name).ok_or_else(|| {
            EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!(
                    "cannot create the field '{}': the name collides with a sibling \
                     or is not one a field can have",
                    draft.name
                ),
            )
        })?;

    let (set, clear) = draft.flags.masks();
    if (set, clear) != (0, 0) && !document.set_field_flags(field_object_number, set, clear) {
        return Err(refused("the field flags"));
    }
    if !draft.options.is_empty() {
        write_options(document, field_object_number, &draft.options)?;
    }
    if let Some(default_value) = draft.default_value.as_ref()
        && !document
            .set_field_default_values(field_object_number, std::slice::from_ref(default_value))
    {
        return Err(refused("the default value"));
    }
    if let Some(max_length) = draft.max_length
        && !document.set_field_max_length(field_object_number, max_length)
    {
        return Err(refused("the maximum length"));
    }
    if let Some(name) = draft.alternate_name.as_deref()
        && !document.set_field_alternate_name(field_object_number, name)
    {
        return Err(refused("the alternate name"));
    }
    if let Some(name) = draft.mapping_name.as_deref()
        && !document.set_field_mapping_name(field_object_number, name)
    {
        return Err(refused("the mapping name"));
    }

    for placement in draft.placements() {
        cancel.check()?;
        let widget_object_number = page.create_widget(document, placement)?;
        let on_state =
            placement.on_state.as_deref().or_else(|| draft.family.default_on_state()).unwrap_or("");
        attach(document, field_object_number, widget_object_number, on_state)?;
    }

    read_field_back(document, field_object_number, cancel)
}

/// How a widget annotation gets made.
///
/// A trait rather than a page handle, because creating one means acquiring
/// a page this layer does not own: the pages a field's widgets land on are
/// the caller's to hold open, and several placements may name several pages.
pub trait WidgetPageSource {
    /// Create, place and style a widget, and return its object number.
    fn create_widget(
        &self,
        document: &Document,
        placement: &WidgetPlacement,
    ) -> Result<u32, EngineError>;
}

/// Change a field.
pub fn update_field(
    document: &Document,
    reference: &FormFieldRef,
    patch: &FormFieldPatch,
    cancel: &CancelToken,
) -> Result<FormFieldDto, EngineError> {
    use crate::dto::Patch;

    cancel.check()?;
    let model = load_model(document)?;
    let resolved = resolve(&model, reference)?;
    let object_number = require_writable(&resolved)?;
    require_unlocked(document, &model, &resolved)?;
    let family = FormFieldFamily::from_code(
        model.field(resolved.index).expect("the index resolved").family_code(),
    );
    if family != patch.family {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!("a {:?} patch cannot be applied to a {family:?} field", patch.family),
        ));
    }
    drop(model);
    cancel.check()?;

    if let Some(name) = patch.name.as_deref()
        && !document.set_field_name(object_number, name)
    {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!("cannot rename the field to '{name}': a sibling already has that name"),
        ));
    }
    let (set, clear) = patch.flags.masks();
    if (set, clear) != (0, 0) && !document.set_field_flags(object_number, set, clear) {
        return Err(refused("the field flags"));
    }
    match &patch.max_length {
        Patch::Keep => {}
        // Clearing the limit is writing zero, which is what an absent
        // `/MaxLen` means: no limit rather than a limit of none.
        Patch::Clear => {
            if !document.set_field_max_length(object_number, 0) {
                return Err(refused("clearing the maximum length"));
            }
        }
        Patch::Set(max_length) => {
            if !document.set_field_max_length(object_number, *max_length) {
                return Err(EngineError::with_message(
                    EngineErrorCode::InvalidArg,
                    "the maximum length is below the length of the value already there",
                ));
            }
        }
    }
    match &patch.default_value {
        Patch::Keep => {}
        Patch::Clear => {
            if !document.remove_field_default_value(object_number) {
                return Err(refused("removing the default value"));
            }
        }
        Patch::Set(value) => {
            if !document.set_field_default_values(object_number, std::slice::from_ref(value)) {
                return Err(refused("the default value"));
            }
        }
    }
    write_patch_name(document, object_number, &patch.alternate_name, true)?;
    write_patch_name(document, object_number, &patch.mapping_name, false)?;
    if let Some(options) = patch.options.as_ref() {
        write_options(document, object_number, options)?;
    }

    read_field_back(document, object_number, cancel)
}

/// `/TU` or `/TM`, tri-state.
///
/// There is no remover for either, so a clear writes the empty string. The
/// reader reports an empty `/TU` as absent, which is what makes the two the
/// same thing to everything downstream.
fn write_patch_name(
    document: &Document,
    object_number: u32,
    value: &crate::dto::Patch<String>,
    alternate: bool,
) -> Result<(), EngineError> {
    use crate::dto::Patch;
    let text = match value {
        Patch::Keep => return Ok(()),
        Patch::Clear => "",
        Patch::Set(text) => text.as_str(),
    };
    let ok = if alternate {
        document.set_field_alternate_name(object_number, text)
    } else {
        document.set_field_mapping_name(object_number, text)
    };
    if !ok {
        return Err(refused(if alternate { "the alternate name" } else { "the mapping name" }));
    }
    Ok(())
}

/// Remove a field, detaching its widgets.
pub fn delete_field(
    document: &Document,
    reference: &FormFieldRef,
    cancel: &CancelToken,
) -> Result<FormFieldDeleteResult, EngineError> {
    cancel.check()?;
    let model = load_model(document)?;
    let resolved = resolve(&model, reference)?;
    let object_number = require_writable(&resolved)?;
    require_unlocked(document, &model, &resolved)?;

    // The widgets are read BEFORE the deletion. Afterwards the field is
    // gone and nothing can say which annotations used to belong to it.
    let field = model.field(resolved.index).expect("the index resolved");
    let before = super::snapshot::read_field(
        &model,
        &field,
        resolved.index,
        &mut ActionReadBudgetTracker::default(),
    )?;
    let widgets: Vec<FormWidgetRef> = before.widgets().cloned().collect();
    drop(model);
    cancel.check()?;

    document.delete_field(object_number).ok_or_else(|| {
        EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "the field cannot be deleted: it has children, or it is not a terminal field",
        )
    })?;
    Ok(FormFieldDeleteResult {
        deleted_field_object_number: object_number,
        detached_widgets: widgets,
    })
}

/// Adopt an existing widget annotation.
pub fn attach_widget(
    document: &Document,
    reference: &FormFieldRef,
    widget_object_number: u32,
    on_state: Option<&str>,
    cancel: &CancelToken,
) -> Result<FormFieldDto, EngineError> {
    cancel.check()?;
    let model = load_model(document)?;
    let resolved = resolve(&model, reference)?;
    let object_number = require_writable(&resolved)?;
    require_unlocked(document, &model, &resolved)?;
    let family = FormFieldFamily::from_code(
        model.field(resolved.index).expect("the index resolved").family_code(),
    );
    drop(model);

    let state = match family {
        FormFieldFamily::Checkbox => on_state.unwrap_or("Yes"),
        FormFieldFamily::Radio => on_state.unwrap_or(""),
        _ => "",
    };
    // A toggle addresses its widgets by state, so one with no state, or one
    // named `Off`, could never be turned on.
    if matches!(family, FormFieldFamily::Checkbox | FormFieldFamily::Radio)
        && (state.is_empty() || state == "Off")
    {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "attaching to a toggle needs an onState that is not empty and not \"Off\"",
        ));
    }
    attach(document, object_number, widget_object_number, state)?;
    read_field_back(document, object_number, cancel)
}

/// Release a widget, leaving it an inert annotation.
pub fn detach_widget(
    document: &Document,
    reference: &FormFieldRef,
    widget_object_number: u32,
    cancel: &CancelToken,
) -> Result<FormFieldDto, EngineError> {
    cancel.check()?;
    let model = load_model(document)?;
    let resolved = resolve(&model, reference)?;
    let object_number = require_writable(&resolved)?;
    require_unlocked(document, &model, &resolved)?;
    drop(model);

    if !document.detach_widget(object_number, widget_object_number) {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "the widget is not attached to this field",
        ));
    }
    read_field_back(document, object_number, cancel)
}

fn attach(
    document: &Document,
    field_object_number: u32,
    widget_object_number: u32,
    on_state: &str,
) -> Result<(), EngineError> {
    let adopted = document
        .attach_widget(field_object_number, widget_object_number, on_state)
        .map_err(|e| {
            EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!("the onState is not a name a PDF can hold: {e}"),
            )
        })?;
    if !adopted {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "the widget cannot be adopted: it belongs to a field already, it is merged \
             with one, or it is not a widget",
        ));
    }
    Ok(())
}

fn write_options(
    document: &Document,
    object_number: u32,
    options: &[FieldOptionInput],
) -> Result<(), EngineError> {
    let labels: Vec<String> = options.iter().map(|o| o.label.clone()).collect();
    let exports: Vec<String> = options.iter().map(|o| o.value.clone()).collect();
    if !document.set_field_options(object_number, &labels, &exports) {
        return Err(refused("the options"));
    }
    Ok(())
}

/// Everything a draft can be wrong about before anything is written.
fn preflight_draft(draft: &FormFieldDraft) -> Result<(), EngineError> {
    if draft.name.is_empty() {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "a form field needs a name",
        ));
    }
    if draft.family == CreatableFamily::Radio {
        for placement in draft.placements() {
            let state = placement.on_state.as_deref().unwrap_or("");
            if state.is_empty() || state == "Off" {
                return Err(EngineError::with_message(
                    EngineErrorCode::InvalidArg,
                    "every radio widget needs an onState that is not empty and not \"Off\": \
                     the group addresses its widgets by it",
                ));
            }
        }
    }
    // Options on a family with none would be written and then never read.
    if !draft.options.is_empty()
        && !matches!(draft.family, CreatableFamily::Combobox | CreatableFamily::Listbox)
    {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!("a {:?} field has no options", draft.family),
        ));
    }
    Ok(())
}

fn refused(what: &str) -> EngineError {
    EngineError::with_message(EngineErrorCode::InvalidArg, format!("PDFium refused {what}"))
}

/// A widget source that opens each placement's page itself.
///
/// The simple answer, and the right one for a caller with no page pool: a
/// field's widgets may land on several pages, each is touched once, and a
/// page held open across the whole operation would be held for the ones it
/// is not on.
pub struct OwnedPageWidgets<'doc> {
    document: &'doc crate::access::OwnedDocument,
}

impl<'doc> OwnedPageWidgets<'doc> {
    pub fn new(document: &'doc crate::access::OwnedDocument) -> Self {
        Self { document }
    }
}

impl WidgetPageSource for OwnedPageWidgets<'_> {
    fn create_widget(
        &self,
        _document: &Document,
        placement: &WidgetPlacement,
    ) -> Result<u32, EngineError> {
        use crate::access::DocumentAccess;

        let page = self.document.page(placement.page_object_number)?;
        create_widget_on(crate::access::PageAccess::view(&page), placement)
    }
}

/// Birth a widget on an already-open page.
///
/// It goes through the ANNOTATION plane, not a forms call: a widget is an
/// annotation, and `EPDFPage_CreateAnnot` is what gives it the indirect
/// object number the adoption will name. Styling it uses the widget
/// annotation kind's own writer, so a widget placed here and one created
/// through `annotations.create` cannot drift.
pub fn create_widget_on(
    page: &pdfium::PageView<'_>,
    placement: &WidgetPlacement,
) -> Result<u32, EngineError> {
    use crate::features::annotation::subtype::AnnotationSubtype;

    let annotation = page.create_annotation(AnnotationSubtype::Widget.code()).ok_or_else(|| {
        EngineError::with_message(
            EngineErrorCode::Unknown,
            "EPDFPage_CreateAnnot returned nothing for a widget",
        )
    })?;
    // The placement's rect wins over any the appearance carried: the
    // placement is where the widget goes and the appearance is how it looks.
    // An absent appearance leaves every widget-plane entry off, which is
    // what an inert widget the field will style is.
    let fields = crate::features::annotation::write::draft::WidgetDraftFields {
        rect: placement.rect,
        style: placement.appearance.clone().unwrap_or_default(),
    };
    crate::features::annotation::write::create::apply_widget(&annotation, &fields)?;

    let object_number = annotation.object_number();
    if object_number == 0 {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "the new widget annotation has no object number",
        ));
    }
    Ok(object_number)
}
