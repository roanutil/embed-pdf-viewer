//! `services/design.ts`: whether the form's widgets are lent to the
//! annotation plugin now (`write/lending.rs`): the session may design, the
//! annotation plugin is there, and the active tool doesn't fill. Then the
//! annotation layer draws and edits the widgets, and the form layer leaves
//! them out.

impl crate::controller::Core {
    /// `design.lending()`.
    pub fn lending(&self) -> bool {
        let Some(interaction) = self.interaction() else { return false };
        self.annotation().is_some()
            && self.allows("doc.forms.modify")
            && !interaction.get_active_tool().enables("form-fill")
    }
}
