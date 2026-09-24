//! `/State` and `/StateModel` spellings.
//!
//! G7. Ports `services/src/features/annotations/internal/annotationState.ts`,
//! which is §12.5.6.3 Table 174.
//!
//! Two maps and four functions, and the interesting property is what they do
//! with a value they do not know: they pass it through verbatim, in both
//! directions. Acrobat lets a workflow register its own state model with its
//! own states, and the PDF stores both as plain text strings. Closing the set
//! would turn somebody else's review workflow into `none` on the first save.
//!
//! So a state is a `String` on the wire, not an enum, and the known values are
//! normalized to lowercase only so that the seven Table 174 states have one
//! spelling for a consumer to switch on.

/// The two standard state models, wire spelling to PDF spelling.
pub const STATE_MODELS: [(&str, &str); 2] = [("review", "Review"), ("marked", "Marked")];

/// The seven standard states, wire spelling to PDF spelling.
///
/// The first five belong to the `review` model and the last two to `marked`.
/// The pairing is not enforced here: a file can hold any combination, and
/// rejecting one would lose data the reader was asked to report faithfully.
pub const STATES: [(&str, &str); 7] = [
    ("accepted", "Accepted"),
    ("rejected", "Rejected"),
    ("cancelled", "Cancelled"),
    ("completed", "Completed"),
    ("none", "None"),
    ("marked", "Marked"),
    ("unmarked", "Unmarked"),
];

fn to_pdf(table: &[(&str, &'static str)], value: &str) -> String {
    table
        .iter()
        .find(|(wire, _)| *wire == value)
        .map_or_else(|| value.to_owned(), |(_, pdf)| (*pdf).to_owned())
}

fn from_pdf(table: &[(&'static str, &str)], value: &str) -> String {
    table
        .iter()
        .find(|(_, pdf)| *pdf == value)
        .map_or_else(|| value.to_owned(), |(wire, _)| (*wire).to_owned())
}

/// Wire `/StateModel` to what the file stores.
pub fn state_model_to_pdf(value: &str) -> String {
    to_pdf(&STATE_MODELS, value)
}

/// What the file stores to the wire `/StateModel`.
pub fn state_model_from_pdf(value: &str) -> String {
    from_pdf(&STATE_MODELS, value)
}

/// Wire `/State` to what the file stores.
pub fn state_to_pdf(value: &str) -> String {
    to_pdf(&STATES, value)
}

/// What the file stores to the wire `/State`.
pub fn state_from_pdf(value: &str) -> String {
    from_pdf(&STATES, value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_standard_spellings_round_trip() {
        for (wire, pdf) in STATE_MODELS {
            assert_eq!(state_model_to_pdf(wire), pdf);
            assert_eq!(state_model_from_pdf(pdf), wire);
        }
        for (wire, pdf) in STATES {
            assert_eq!(state_to_pdf(wire), pdf);
            assert_eq!(state_from_pdf(pdf), wire);
        }
    }

    #[test]
    fn a_custom_workflows_vocabulary_survives_a_round_trip() {
        for custom in ["Escalated", "legal-hold", "", "Review "] {
            assert_eq!(state_model_from_pdf(&state_model_to_pdf(custom)), custom);
            assert_eq!(state_from_pdf(&state_to_pdf(custom)), custom);
        }
    }

    #[test]
    fn marked_is_in_both_tables_and_means_different_things() {
        // `Marked` is a state model AND a state of that model, so the two
        // tables have to stay separate: one map would make `marked` ambiguous.
        assert_eq!(state_model_to_pdf("marked"), "Marked");
        assert_eq!(state_to_pdf("marked"), "Marked");
        // And the state model has no `unmarked` entry, so it passes through.
        assert_eq!(state_model_to_pdf("unmarked"), "unmarked");
        assert_eq!(state_to_pdf("unmarked"), "Unmarked");
    }

    #[test]
    fn a_pdf_spelling_offered_as_a_wire_value_is_not_translated_twice() {
        // `to_pdf` matches on the wire column only, so the already-capitalized
        // value passes through rather than matching itself.
        assert_eq!(state_to_pdf("Accepted"), "Accepted");
        assert_eq!(state_from_pdf("accepted"), "accepted");
    }
}
