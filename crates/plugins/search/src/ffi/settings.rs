//! Live scalar settings; registration resources stay owned by Rust.

use super::*;

pub use crate::SearchBlendMode;
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SearchHighlightSettings {
    pub color: String,
    pub active_color: String,
    pub blend_mode: SearchBlendMode,
}
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SearchRevealSettings {
    pub anchor: Option<SearchRevealAnchor>,
    pub behavior: SearchScrollBehavior,
}
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SearchSettings {
    pub reveal: SearchRevealSettings,
    pub highlight: SearchHighlightSettings,
}
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct SearchHighlightPatch {
    #[uniffi(default = None)]
    pub color: Option<String>,
    #[uniffi(default = None)]
    pub active_color: Option<String>,
    #[uniffi(default = None)]
    pub blend_mode: Option<SearchBlendMode>,
}
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct SearchRevealPatch {
    #[uniffi(default = None)]
    pub anchor: Option<SearchRevealAnchor>,
    #[uniffi(default = false)]
    pub clear_anchor: bool,
    #[uniffi(default = None)]
    pub behavior: Option<SearchScrollBehavior>,
}
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct SearchSettingsPatch {
    #[uniffi(default = None)]
    pub reveal: Option<SearchRevealPatch>,
    #[uniffi(default = None)]
    pub highlight: Option<SearchHighlightPatch>,
}
impl From<SearchSettingsPatch> for crate::SearchConfig {
    fn from(patch: SearchSettingsPatch) -> Self {
        Self {
            reveal: patch.reveal.map(|reveal| crate::SearchRevealConfig {
                align: if reveal.clear_anchor {
                    Some(None)
                } else {
                    reveal.anchor.map(|anchor| Some(anchor.into()))
                },
                behavior: reveal.behavior.map(Into::into),
            }),
            highlight: patch.highlight.map(|highlight| crate::SearchHighlightConfig {
                color: highlight.color,
                active_color: highlight.active_color,
                blend_mode: highlight.blend_mode,
            }),
        }
    }
}
impl From<RevealAnchorValue> for SearchRevealAxis {
    fn from(axis: RevealAnchorValue) -> Self {
        match axis {
            RevealAnchorValue::Word(RevealWord::Keep) => Self::Keep,
            RevealAnchorValue::Word(RevealWord::Start) => Self::Start,
            RevealAnchorValue::Word(RevealWord::Center) => Self::Center,
            RevealAnchorValue::Word(RevealWord::End) => Self::End,
            RevealAnchorValue::Fraction(value) => Self::Fraction { value },
        }
    }
}
impl From<ScrollBehaviorKind> for SearchScrollBehavior {
    fn from(behavior: ScrollBehaviorKind) -> Self {
        match behavior {
            ScrollBehaviorKind::Smooth => Self::Smooth,
            ScrollBehaviorKind::Instant => Self::Instant,
        }
    }
}
#[uniffi::export]
impl SearchHandle {
    /// Current settings shared by this plugin registration.
    pub async fn settings(&self) -> Result<SearchSettings, ViewerError> {
        self.cap
            .call(|c| {
                let settings = open(c)?.get_settings();
                Ok(SearchSettings {
                    reveal: SearchRevealSettings {
                        anchor: settings.reveal.align.map(|align| SearchRevealAnchor {
                            x: align.x.map(Into::into),
                            y: align.y.map(Into::into),
                        }),
                        behavior: settings.reveal.behavior.into(),
                    },
                    highlight: SearchHighlightSettings {
                        color: settings.highlight.color,
                        active_color: settings.highlight.active_color,
                        blend_mode: settings.highlight.blend_mode,
                    },
                })
            })
            .await
    }
    /// Merge only the named fields, preserving registration resources.
    pub async fn update_settings(&self, patch: SearchSettingsPatch) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?.update_settings(&(crate::SearchConfig::from(patch)));
                Ok(())
            })
            .await
    }
    /// Restore the values supplied when the plugin was registered.
    pub async fn reset_settings(&self) -> Result<(), ViewerError> {
        self.cap
            .call(|c| {
                open(c)?.reset_settings();
                Ok(())
            })
            .await
    }
}
