//! `services/policy.ts`: the policy matrix (type × origin → decision), the
//! defaults, the fixed `never` and unsupported sets, the row-wise merge, and
//! the live policy with its one decision function.

use serde_json::Value;

use super::str_of;
use crate::ActionsCapability;
use crate::contract::{
    ActionOrigin, ActionPolicy, ActionPolicyDecision, ActionPolicyPatch, ActionPolicyRow,
};
use crate::model::ActionsAction;

const ALLOW_ALL: ActionPolicyRow = ActionPolicyRow {
    user: ActionPolicyDecision::Allow,
    hover: ActionPolicyDecision::Allow,
    lifecycle: ActionPolicyDecision::Allow,
};
const ADAPTER_ON_USER_BLOCK_ELSE: ActionPolicyRow = ActionPolicyRow {
    user: ActionPolicyDecision::Adapter,
    hover: ActionPolicyDecision::Block,
    lifecycle: ActionPolicyDecision::Block,
};

/// Never executable, not configurable: diagnostics only.
const NEVER_TYPES: &[&str] =
    &["launch", "goto-remote", "goto-embedded", "sound", "movie", "import-data"];
/// Recognized types with no interpreter.
const UNSUPPORTED_TYPES: &[&str] =
    &["rendition", "thread", "set-ocg-state", "transition", "goto-3d-view", "unknown"];
/// Document-lifetime node types: engine mutations, committed in walk order.
pub(crate) const DOCUMENT_TYPES: &[&str] = &["reset-form", "javascript"];

/// `DEFAULT_POLICY`.
pub fn default_policy() -> ActionPolicy {
    ActionPolicy {
        goto: ALLOW_ALL,
        named: ALLOW_ALL,
        hide: ALLOW_ALL,
        reset_form: ALLOW_ALL,
        javascript: ALLOW_ALL,
        // No auto-opened tabs: only a real user activation reaches the adapter.
        uri: ActionPolicyRow {
            user: ActionPolicyDecision::Adapter,
            hover: ActionPolicyDecision::Report,
            lifecycle: ActionPolicyDecision::Report,
        },
        // The Named Print verb: adapter on user gestures, blocked otherwise.
        print: ADAPTER_ON_USER_BLOCK_ELSE,
        // The sink chain on user gestures only.
        submit_form: ADAPTER_ON_USER_BLOCK_ELSE,
    }
}

/// `mergePolicy(base, patch)`: row-wise; a patch names only the origins it
/// changes, and an absent or empty row is left alone.
pub fn merge_policy(base: &ActionPolicy, patch: Option<&ActionPolicyPatch>) -> ActionPolicy {
    let Some(patch) = patch else { return base.clone() };
    let mut next = base.clone();
    let pick = |row: &mut ActionPolicyRow, over: &Option<_>| {
        if let Some(over) = over {
            *row = row.merged(over);
        }
    };
    pick(&mut next.goto, &patch.goto);
    pick(&mut next.named, &patch.named);
    pick(&mut next.hide, &patch.hide);
    pick(&mut next.reset_form, &patch.reset_form);
    pick(&mut next.javascript, &patch.javascript);
    pick(&mut next.uri, &patch.uri);
    pick(&mut next.print, &patch.print);
    pick(&mut next.submit_form, &patch.submit_form);
    next
}

/// `isPrintVerb(node)`.
pub(crate) fn is_print_verb(node: &Value) -> bool {
    str_of(node, "type") == "named" && str_of(node, "name") == "Print"
}

/// A decision, or one of the two fixed verdicts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Decision {
    Policy(ActionPolicyDecision),
    Never,
    Unsupported,
}

impl ActionsCapability {
    /// `decisionFor(node, origin)`, against the live policy.
    pub(crate) fn decision_for(&self, node: &Value, origin: ActionOrigin) -> Decision {
        let kind = str_of(node, "type");
        if NEVER_TYPES.contains(&kind) {
            return Decision::Never;
        }
        if UNSUPPORTED_TYPES.contains(&kind) {
            return Decision::Unsupported;
        }
        let policy = self.0.policy.borrow();
        if is_print_verb(node) {
            return Decision::Policy(policy.print.get(origin));
        }
        match policy.row(kind) {
            Some(row) => Decision::Policy(row.get(origin)),
            None => Decision::Unsupported,
        }
    }

    /// The effective policy. A snapshot: [`update_policy`](Self::update_policy)
    /// replaces it.
    pub fn get_policy(&self) -> ActionPolicy {
        self.0.policy.borrow().clone()
    }

    /// Live policy change: rows merge over the current policy, and later
    /// dispatches decide with the new one.
    pub fn update_policy(&self, patch: &ActionPolicyPatch) {
        let next = merge_policy(&self.0.policy.borrow(), Some(patch));
        *self.0.policy.borrow_mut() = next;
        self.0.cx.dispatch(ActionsAction::ActionsPolicyChanged);
    }
}
