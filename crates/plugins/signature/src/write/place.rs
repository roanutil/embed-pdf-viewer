//! `write/place.ts`: the destination rule and the sign-here flow. A mark
//! over a field is signed, drawn, or handed to the chrome by mode; a free
//! placement is a stamp (one placement law, the stamp plugin's).

use std::rc::Rc;

use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::rt::Promise;
use epdf_plugin_annotation::write::stamps::StampInput;
use epdf_plugin_stamp::StampDocumentOptions;
use serde_json::Value;

use crate::contract::*;
use crate::controller::SignatureCapability;
use crate::js::strict_eq;
use crate::model::{has_signed_field, same_field_ref, set_target};
use crate::services::errors::verb;

fn ref_of(placed: &Value) -> Value {
    placed
        .get("annotation")
        .and_then(|a| a.get("ref"))
        .cloned()
        .unwrap_or_else(epdf_kernel::undefined)
}

impl SignatureCapability {
    /// `sameField(left, right)`: one field, whether a ref names it by
    /// object number or by name.
    fn same_field(&self, left: &Value, right: &Value) -> bool {
        if same_field_ref(left, right) {
            return true;
        }
        let Some(signature) = self.get_signature(left) else { return false };
        let other = self.get_signature(right);
        strict_eq(signature.get("index"), other.as_ref().and_then(|o| o.get("index")))
    }

    /// `clearIfTarget(field)`: a field that was just signed or filled is no
    /// longer the target.
    pub(crate) fn clear_if_target(&self, field: &Value) {
        let current = self.state().target.clone();
        if current.is_some_and(|current| self.same_field(&current, field)) {
            self.update_state(|s| set_target(s, None));
        }
    }

    /// `setTarget(field)`: make a field the target, where the next mark the
    /// person picks goes; `None` clears. Fires `onTargetChanged`.
    pub fn set_target(&self, field: Option<Value>) {
        self.update_state(|s| set_target(s, field.map(Rc::new)));
    }

    /// `requestInspection(field)`: ask the UI to show a signed field's
    /// details. Fires `onInspectionRequested`.
    pub fn request_inspection(&self, field: Value) {
        self.0.events.inspection_requested.emit(&SignatureInspectionRequestedEvent { field });
    }

    /// `placeMark(mark, target, options)`: what a click with an armed mark
    /// does. In a field, sign, fill or ask by mode; anywhere else, place it
    /// as a stamp.
    pub fn place_mark(
        &self,
        mark: Mark,
        target: PlaceTarget,
        options: OperationOptions,
    ) -> Promise<PlaceMarkResult> {
        let this = self.clone();
        verb(Promise::spawn(async move {
            let field = match target {
                PlaceTarget::Free(placement) => {
                    // Free placement: a stamp, as in Preview; one placement
                    // law, the stamp plugin's.
                    let placed = match &mark {
                        Mark::Asset { asset_id } => {
                            let Some(library) = this.stamp() else {
                                return Err(plugin_error(
                                    code::UNSUPPORTED,
                                    "signature",
                                    "no stamp plugin to place an asset",
                                    None,
                                    None,
                                ));
                            };
                            let document_id = this.document_id()?;
                            library
                                .place_asset(
                                    asset_id,
                                    placement,
                                    StampDocumentOptions {
                                        document_id: Some(document_id),
                                        signal: options.signal.clone(),
                                    },
                                )
                                .await?
                        }
                        Mark::Source { source } => {
                            let Some(annotations) = this.annotation() else {
                                return Err(plugin_error(
                                    code::UNSUPPORTED,
                                    "signature",
                                    "no annotation plugin to place a mark",
                                    None,
                                    None,
                                ));
                            };
                            let input = StampInput::read(&serde_json::json!({ "source": source }));
                            annotations
                                .stamps_place(input, &placement, options.signal.clone())
                                .await?
                        }
                    };
                    return Ok(PlaceMarkResult::Placed { annotation: ref_of(&placed) });
                }
                PlaceTarget::Field(field) => field,
            };
            match this.get_mode() {
                SignatureMode::Sign => {
                    // The first signature is the one that can certify: when
                    // the settings allow a certification, let the chrome
                    // offer the choice instead of sealing a plain approval.
                    if this.settings().allow_certify && !has_signed_field(this.snapshot().as_ref())
                    {
                        this.0
                            .events
                            .sign_requested
                            .emit(&SignatureSignRequestedEvent { field: field.clone(), mark });
                        return Ok(PlaceMarkResult::Requested { field });
                    }
                    let result =
                        this.sign_raw(SignFieldInput::new(field.clone(), mark), options).await?;
                    Ok(PlaceMarkResult::Signed { field, result })
                }
                SignatureMode::Visual => {
                    this.fill_field_raw(field.clone(), mark, options).await?;
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
        }))
    }
}
