//! `scripting/surface.ts`: the one UI and diagnostic port for script
//! results. Print and submit effects from the actions plane never reach this
//! door: the executor routes them after the transaction. Effects here come
//! from the form pipeline's K/V/C/F path and from detached realms; a print
//! goes to the kernel's print when the host gave a printer.
//!
//! The form pipeline's script print is not checked against the `print`
//! policy row here, as the TypeScript does not check it (`surface.ts`).
//! Whether to close that gap is the Task 14 decision; M14's check lives on
//! the executor path only.

use epdf_acrojs::{ScriptDiagnostic, ScriptDiagnosticCode, ScriptUiEffect};
use epdf_kernel::Error;
use epdf_kernel::rt;

use crate::ActionsCapability;
use crate::contract::{
    ActionContext, ActionDiagnostic, ActionDiagnosticCode, ActionOrigin, ActionSource,
    ActionTriggerEvent, ActionUiContext, AlertOptions, ScriptFailedEvent, ScriptPhase,
};
use crate::host_contract::{
    ScriptCommitContext, ScriptCommitSurface, ScriptRealmKind, ScriptSurfaceResult,
};
use crate::services::DiagnosticPlace;
use crate::submit::intent::intent_of_submit_effect;

impl ActionsCapability {
    fn suppress(&self, message: String) {
        self.0
            .events
            .script_diagnostic_reported
            .emit(&ScriptDiagnostic { code: ScriptDiagnosticCode::UiEffectSuppressed, message });
    }

    /// Host lens: surface a script transaction's UI effects, diagnostics and
    /// error through the one port (the adapter, the authority print gate and
    /// the script hooks). `Err` is an adapter that threw.
    pub fn surface_script_result(&self, result: &ScriptSurfaceResult) -> Result<(), Error> {
        let ui_context = ActionUiContext { origin: result.origin, phase: result.phase };
        let source = result.source.clone();
        let place = DiagnosticPlace { action: Some("javascript".into()), source: source.clone() };
        for effect in &result.ui_effects {
            // A detached realm has no document surface: only alerts have a
            // valid target (the user); the rest are suppressed, observably.
            if result.realm == ScriptRealmKind::Detached
                && !matches!(effect, ScriptUiEffect::Alert { .. })
            {
                self.suppress(format!(
                    "script {} request from a detached realm suppressed: no document surface",
                    effect.kind()
                ));
                continue;
            }
            if let Some(intent) = intent_of_submit_effect(effect) {
                // A form-pipeline scripted submit: the same door, not awaited
                // (the form queue must not wait on submit sinks).
                let ctx = ActionContext {
                    origin: result.origin,
                    source: source.clone().unwrap_or(ActionSource::Api),
                    event: ActionTriggerEvent::Activate,
                };
                let (this, diagnose) = (self.clone(), self.reporter(place.clone()));
                rt::spawn(async move {
                    this.perform_submit(intent, ctx, diagnose).await;
                });
                continue;
            }
            let is_print = matches!(effect, ScriptUiEffect::Print);
            // Permission, not preference.
            if is_print && !self.allows_print() {
                self.suppress("script print request withheld: doc.print is not allowed".into());
                continue;
            }
            // The latch: one dialog per outer request.
            if is_print && self.0.print_latch.get() {
                self.suppress(
                    "script print request during a document print event — suppressed (reentrant)"
                        .into(),
                );
                continue;
            }
            // With the kernel's print, a user's print request prints the
            // document, and the kernel runs /WP and /DP around it. A hover or
            // lifecycle request never prints.
            let printer = self.0.document_print.clone();
            if is_print && (printer.available)() {
                if result.origin != ActionOrigin::User {
                    self.suppress(format!(
                        "script print request from a {} event suppressed",
                        result.origin.as_str()
                    ));
                    continue;
                }
                let (started, this, place) = ((printer.start)(), self.clone(), place.clone());
                rt::spawn(async move {
                    if let Err(error) = started.reaction().await {
                        this.report_diagnostic(
                            &ActionDiagnostic::new(
                                ActionDiagnosticCode::TriggerFailed,
                                format!("print: {}", error.message),
                            ),
                            &place,
                        );
                    }
                });
                continue;
            }
            let adapter = self.0.ports.ui_adapter.borrow().clone();
            let Some(adapter) = adapter else {
                self.report_diagnostic(
                    &ActionDiagnostic::new(
                        ActionDiagnosticCode::NoAdapter,
                        format!("script {}: no UI adapter installed", effect.kind()),
                    ),
                    &place,
                );
                continue;
            };
            match effect {
                ScriptUiEffect::Alert { message, icon, title } => {
                    if let Some(alert) = &adapter.alert {
                        alert(
                            message,
                            &AlertOptions {
                                origin: ui_context.origin,
                                phase: ui_context.phase,
                                icon: *icon,
                                title: title.clone(),
                            },
                        )?;
                    }
                }
                ScriptUiEffect::GotoPage { page } => {
                    if let Some(goto_page) = &adapter.goto_page {
                        goto_page(*page, &ui_context)?;
                    }
                }
                _ => (adapter.print)(Some(&ui_context))?,
            }
        }
        for diagnostic in &result.diagnostics {
            self.0.events.script_diagnostic_reported.emit(diagnostic);
        }
        if let Some(error) = &result.error {
            self.0.events.script_failed.emit(&ScriptFailedEvent { error: error.clone(), source });
        }
        Ok(())
    }

    /// Host lens: surface a K/V/C/F commit as up to two results, the boot
    /// phase (only when it produced effects) and the user phase (always: it
    /// carries the diagnostics and the error).
    pub fn surface_script_commit(
        &self,
        commit: &ScriptCommitSurface,
        context: &ScriptCommitContext,
    ) -> Result<(), Error> {
        for phase in [ScriptPhase::Boot, ScriptPhase::User] {
            let ui_effects: Vec<ScriptUiEffect> = commit
                .ui_effects
                .iter()
                .filter(|e| e.phase == phase)
                .map(|e| e.effect.clone())
                .collect();
            if ui_effects.is_empty() && phase == ScriptPhase::Boot {
                continue;
            }
            let user = phase == ScriptPhase::User;
            self.surface_script_result(&ScriptSurfaceResult {
                ui_effects,
                diagnostics: if user { commit.diagnostics.clone() } else { Vec::new() },
                error: if user { commit.error.clone() } else { None },
                origin: context.origin,
                phase,
                realm: context.realm,
                source: context.source.clone(),
            })?;
        }
        Ok(())
    }
}
