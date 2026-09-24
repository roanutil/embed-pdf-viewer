//! The `/IT` intent names.
//!
//! G7. Ports `services/src/features/annotations/internal/{freeTextIntent,
//! inkIntent,textEditIntent}.ts`.
//!
//! `/IT` is a name, not a code, so these are string maps rather than integer
//! ones, and they behave differently from the codecs in
//! [`super::vocabulary`] in a way worth stating: three of the four decode to
//! `Option`, because an ink annotation with no `/IT` is an ordinary ink
//! annotation and there is nothing to default to. Free text is the exception.
//! It has a meaningful default, so it decodes to a value.

use serde::{Deserialize, Serialize};

/// Free-text `/IT` (§12.5.6.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FreeTextIntent {
    /// A plain text box.
    FreeText,
    /// A text box with a `/CL` leader line pointing at what it calls out.
    FreeTextCallout,
}

impl FreeTextIntent {
    pub const ALL: [Self; 2] = [Self::FreeText, Self::FreeTextCallout];

    /// Decode an `/IT` name, absent included.
    ///
    /// `FreeTextTypeWriter` reads as `FreeText`. It is a real Acrobat variant
    /// and this engine does not author it, so preserving it on the wire would
    /// promise a round trip the writer cannot keep.
    pub fn from_name(name: Option<&str>) -> Self {
        if name == Some("FreeTextCallout") { Self::FreeTextCallout } else { Self::FreeText }
    }

    /// The `/IT` name a write path stores.
    pub fn name(self) -> &'static str {
        match self {
            Self::FreeText => "FreeText",
            Self::FreeTextCallout => "FreeTextCallout",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::FreeText => "free-text",
            Self::FreeTextCallout => "free-text-callout",
        }
    }
}

/// Ink `/IT`: the Acrobat-compatible freehand highlighter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InkIntent {
    InkHighlight,
}

impl InkIntent {
    pub const ALL: [Self; 1] = [Self::InkHighlight];

    pub fn from_name(name: Option<&str>) -> Option<Self> {
        match name {
            Some("InkHighlight") => Some(Self::InkHighlight),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::InkHighlight => "InkHighlight",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::InkHighlight => "ink-highlight",
        }
    }
}

/// Caret `/IT`: the caret half of a replace-text edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CaretIntent {
    Replace,
}

impl CaretIntent {
    pub const ALL: [Self; 1] = [Self::Replace];

    pub fn from_name(name: Option<&str>) -> Option<Self> {
        match name {
            Some("Replace") => Some(Self::Replace),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Replace => "Replace",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Replace => "replace",
        }
    }
}

/// Strikeout `/IT`: the subordinate half of that same edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StrikeoutIntent {
    StrikeoutTextEdit,
}

impl StrikeoutIntent {
    pub const ALL: [Self; 1] = [Self::StrikeoutTextEdit];

    pub fn from_name(name: Option<&str>) -> Option<Self> {
        match name {
            Some("StrikeOutTextEdit") => Some(Self::StrikeoutTextEdit),
            _ => None,
        }
    }

    /// Note the capital `O`. Acrobat writes `StrikeOutTextEdit` while the
    /// subtype is `StrikeOut` and the wire name is `strikeout`, so all three
    /// spellings differ and only this one goes in the file.
    pub fn name(self) -> &'static str {
        match self {
            Self::StrikeoutTextEdit => "StrikeOutTextEdit",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::StrikeoutTextEdit => "strikeout-text-edit",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_intent_round_trips_through_its_pdf_name() {
        for intent in FreeTextIntent::ALL {
            assert_eq!(FreeTextIntent::from_name(Some(intent.name())), intent);
        }
        for intent in InkIntent::ALL {
            assert_eq!(InkIntent::from_name(Some(intent.name())), Some(intent));
        }
        for intent in CaretIntent::ALL {
            assert_eq!(CaretIntent::from_name(Some(intent.name())), Some(intent));
        }
        for intent in StrikeoutIntent::ALL {
            assert_eq!(StrikeoutIntent::from_name(Some(intent.name())), Some(intent));
        }
    }

    #[test]
    fn an_absent_it_is_nothing_except_on_free_text() {
        assert_eq!(FreeTextIntent::from_name(None), FreeTextIntent::FreeText);
        assert_eq!(InkIntent::from_name(None), None);
        assert_eq!(CaretIntent::from_name(None), None);
        assert_eq!(StrikeoutIntent::from_name(None), None);
    }

    #[test]
    fn the_names_are_matched_exactly_and_not_by_case() {
        assert_eq!(FreeTextIntent::from_name(Some("freetextcallout")), FreeTextIntent::FreeText);
        assert_eq!(InkIntent::from_name(Some("inkhighlight")), None);
        assert_eq!(StrikeoutIntent::from_name(Some("StrikeoutTextEdit")), None);
        // The typewriter variant, which reads as the plain intent by design.
        assert_eq!(FreeTextIntent::from_name(Some("FreeTextTypeWriter")), FreeTextIntent::FreeText);
    }
}
