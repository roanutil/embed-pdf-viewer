//! `connect.ts`: wiring to sibling plugins. With the interaction hub and the
//! stamp plugin present, an armed mark from a `signatures` library dropped
//! over an unsigned signature field goes into the field instead of onto the
//! page.

use std::rc::Rc;

use epdf_kernel::Error;
use epdf_plugin_form::FORM;
use epdf_plugin_interaction::{INTERACTION, RegisterHandlerOptions};
use epdf_plugin_stamp::STAMP;

use crate::controller::SignatureCapability;
use crate::tools::armed_mark::ArmedMarkHandler;

impl SignatureCapability {
    /// `connectSiblings(ctx, signature)`.
    pub(crate) fn connect_siblings(&self) -> Result<(), Error> {
        let interaction = self.cx().try_get(&INTERACTION)?;
        let stamp = self.cx().try_get(&STAMP)?;
        let document_id = self.cx().document_id().map(str::to_string);
        let (Some(interaction), Some(stamp), Some(document_id)) = (interaction, stamp, document_id)
        else {
            return Ok(());
        };
        let handler =
            ArmedMarkHandler::new(document_id, self.clone(), self.cx().get(&FORM)?, stamp);
        let unregister =
            interaction.register_handler(Rc::new(handler), RegisterHandlerOptions::default());
        self.cx().defer(Box::new(move || {
            unregister.call();
            Ok(None)
        }));
        Ok(())
    }
}
