//! `sync/overlay.ts`: the `overlay` settings as the redact tool's defaults.
//! Every mark takes its look from them: one drawn with the tool, one made
//! from the selected text, and one made from code (`write/marks.rs`).
//! Written when the plugin connects and again whenever the settings change,
//! so a change applies to the marks made from then on; marks already made
//! keep theirs.

use epdf_kernel::Error;
use serde_json::{Map, Value};

use crate::controller::RedactionCapability;

/// `toolDefaultsOf(settings)`: the redact tool's fields the overlay
/// settings stand for, read from the settings JSON as the TypeScript reads
/// them (a member the store does not hold is left out).
fn tool_defaults_of(settings: &Value) -> Map<String, Value> {
    let overlay = settings.get("overlay");
    let text = overlay.and_then(|o| o.get("text"));
    let mut out = Map::new();
    for (key, value) in [
        ("interiorColor", overlay.and_then(|o| o.get("fill"))),
        ("fontColor", text.and_then(|t| t.get("color"))),
        ("fontFamily", text.and_then(|t| t.get("fontFamily"))),
        ("fontSize", text.and_then(|t| t.get("fontSize"))),
    ] {
        out.insert(key.into(), value.cloned().unwrap_or_else(epdf_kernel::undefined));
    }
    out
}

impl RedactionCapability {
    /// `syncOverlay(ctx, services)`: run once, from the plugin's `connect`.
    pub(crate) fn sync_overlay(&self) -> Result<(), Error> {
        let core = self.core();
        let write = {
            let this = self.clone();
            move || {
                let core = this.core();
                let defaults = tool_defaults_of(&core.settings.get());
                core.siblings.annotation.tools_update_defaults("redact", &defaults)
            }
        };
        write()?;
        let off = core.settings.events.on(move |event| {
            if event.changed.iter().any(|member| member == "overlay") {
                write()?;
            }
            Ok(())
        });
        core.cx.listen(off);
        Ok(())
    }
}
