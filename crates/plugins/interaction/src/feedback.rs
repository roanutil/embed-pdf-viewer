//! `feedback.ts` and `feedback.types.ts`: platform feedback (haptics), the
//! output half of the interaction seam. One workspace-scoped provider, many
//! consumers; without a provider every call is a no-op, so consumers never
//! branch. The vocabulary is the platform's haptic taxonomy.

use std::rc::Rc;
use std::sync::LazyLock;

use epdf_kernel::{AnyPlugin, Token, define, undefined};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImpactWeight {
    Light,
    Medium,
    Heavy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NotifyKind {
    Success,
    Warning,
    Error,
}

/// `PlatformFeedback`: what a host injects.
pub trait PlatformFeedback {
    fn selection(&self);
    fn impact(&self, weight: Option<ImpactWeight>);
    fn notify(&self, kind: NotifyKind);
}

/// The feedback capability: the provider, or the no-op.
#[derive(Clone)]
pub struct Feedback(pub Rc<dyn PlatformFeedback>);

impl Feedback {
    pub fn selection(&self) {
        self.0.selection();
    }
    pub fn impact(&self, weight: Option<ImpactWeight>) {
        self.0.impact(weight);
    }
    pub fn notify(&self, kind: NotifyKind) {
        self.0.notify(kind);
    }
}

pub static FEEDBACK: LazyLock<Token<Feedback>> = LazyLock::new(|| Token::new("feedback"));

#[derive(Clone, Default)]
pub struct FeedbackPluginOptions {
    pub provider: Option<Rc<dyn PlatformFeedback>>,
}

struct Noop;

impl PlatformFeedback for Noop {
    fn selection(&self) {}
    fn impact(&self, _weight: Option<ImpactWeight>) {}
    fn notify(&self, _kind: NotifyKind) {}
}

/// `feedbackPlugin(options)`: the workspace's one feedback provider.
/// Stateless: the capability is the provider, or the no-op.
pub fn feedback_plugin(options: FeedbackPluginOptions) -> AnyPlugin {
    define::<Value, Value>("feedback")
        .initial_state(undefined)
        .create(&FEEDBACK, move |_| {
            let provider = options.provider.clone().unwrap_or_else(|| Rc::new(Noop));
            Ok((Feedback(provider), None))
        })
        .build()
}
