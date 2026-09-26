//! `dispatch/fold.ts`: aggregate step statuses. A step failure never skips
//! its siblings; this only folds what each step reported. Pure.

use crate::contract::{
    ActionDiagnostic, ActionResultStatus, ActionStepResult, ActionTriggerResult,
};

/// `foldSteps(steps, diagnostics)`.
pub fn fold_steps(
    steps: Vec<ActionStepResult>,
    diagnostics: Vec<ActionDiagnostic>,
) -> ActionTriggerResult {
    let all = |s: ActionResultStatus| steps.iter().all(|step| step.result.status == s);
    let status = if steps.is_empty() {
        ActionResultStatus::Inert
    } else if all(ActionResultStatus::Refused) {
        ActionResultStatus::Refused
    } else if all(ActionResultStatus::Inert) {
        ActionResultStatus::Inert
    } else if all(ActionResultStatus::Executed) {
        ActionResultStatus::Executed
    } else {
        ActionResultStatus::Partial
    };
    ActionTriggerResult { status, steps, diagnostics }
}
