//! `write/place.ts`: the destination rule and the sign-here flow. A mark
//! over a field is signed, drawn, or handed to the chrome by mode; a free
//! placement is a stamp (one placement law, the stamp plugin's).

use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::js::to_value;
use epdf_kernel::rt::Promise;
use epdf_plugin_annotation::StampToolInput;
use serde_json::Value;

use crate::contract::*;
use crate::controller::SignatureCapability;
use crate::model::SignatureAction;
use crate::read::signatures::any_signed;
use crate::services::store::same_ref;

impl SignatureCapability {
    /// `setTarget(field)`: name the field the next picked mark goes to;
    /// null clears. The same ref again is no change.
    pub fn set_target(&self, field: Option<Value>) {
        let current = self.state().target.clone();
        let unchanged = match (&current, &field) {
            (None, None) => true,
            (Some(current), Some(field)) => same_ref(current, field),
            _ => false,
        };
        if unchanged {
            return;
        }
        self.dispatch(SignatureAction::Target { field: field.clone() });
        self.0
            .events
            .target_changed
            .emit(&SignatureTargetChangedEvent { field: field.unwrap_or(Value::Null) });
    }

    /// `clearIfTarget(field)`: a field just signed or filled is no longer
    /// the target.
    pub(crate) fn clear_if_target(&self, field: &Value) {
        let current = self.state().target.clone();
        if current.is_some_and(|c| same_ref(&c, field)) {
            self.set_target(None);
        }
    }

    /// `requestInspection(field)`: ask the host UI to show a signed field's
    /// facts.
    pub fn request_inspection(&self, field: Value) {
        self.0.events.inspection_requested.emit(&SignatureInspectionRequestedEvent { field });
    }

    /// `placeMark(mark, target)`: into a field by mode, or a free
    /// placement as a stamp.
    pub fn place_mark(&self, mark: Mark, target: PlaceTarget) -> Promise<PlaceMarkResult> {
        let this = self.clone();
        Promise::spawn(async move {
            let field = match target {
                PlaceTarget::Free(placement) => {
                    // A stamp, as in Preview: one placement law, the stamp
                    // plugin's.
                    let annotation = match &mark {
                        Mark::Asset { asset_id } => {
                            let Some(library) = this.stamp()? else {
                                return Err(plugin_error(
                                    code::UNSUPPORTED,
                                    "signature",
                                    "no stamp plugin to place an asset",
                                    None,
                                    None,
                                ));
                            };
                            let placed =
                                library.place_asset(&this.document_id()?, asset_id, placement);
                            placed.await?
                        }
                        Mark::Source { source } => {
                            let input = StampToolInput {
                                source: source.clone(),
                                ..StampToolInput::default()
                            };
                            this.annotation()?.place_stamp(input, placement).await?
                        }
                    };
                    return Ok(PlaceMarkResult::Placed { annotation: to_value(&annotation) });
                }
                PlaceTarget::Field(field) => field,
            };
            match this.get_mode() {
                SignatureMode::Sign => {
                    // The first signature is the one that can certify: when
                    // the config allows a certification, let the chrome offer
                    // the choice instead of sealing a plain approval.
                    if this.config().allow_certify == Some(true)
                        && !any_signed(this.state().snapshot.as_ref())
                    {
                        this.0
                            .events
                            .sign_requested
                            .emit(&SignatureSignRequestedEvent { field: field.clone(), mark });
                        return Ok(PlaceMarkResult::Requested { field });
                    }
                    let result = this.sign(SignFieldInput::new(field.clone(), mark)).await?;
                    Ok(PlaceMarkResult::Signed { field, result })
                }
                SignatureMode::Visual => {
                    this.fill_field(field.clone(), mark).await?;
                    Ok(PlaceMarkResult::Filled { field })
                }
                SignatureMode::Ask => {
                    this.0
                        .events
                        .sign_requested
                        .emit(&SignatureSignRequestedEvent { field: field.clone(), mark });
                    Ok(PlaceMarkResult::Requested { field })
                }
            }
        })
    }
}
