//! `packages/engine/core/src/signature/analysis/`: the pure rule engine
//! that judges what changed after a signature. JSON in, JSON out; the
//! engine side that feeds it the fork's object diff is `analyzer.rs`.

pub mod evaluate;
pub mod pdf_value;
pub mod resolve;
pub mod restrictions;
pub mod types;
pub mod verdict;

#[cfg(test)]
mod tests;

pub use evaluate::{evaluate_step, restrictions_of, same_effective_value};
pub use pdf_value::{PdfValue, parse_pdf_value, pdf_value_equals, stable_stringify};
pub use resolve::{DEFAULT_EDGE_RESOLVER_BUDGET, EdgeResolver, EdgeResolverBudget};
pub use restrictions::{Restrictions, restrictions_for};
pub use verdict::{assessment_of, combine, conclude, primary_finding, worst_verdict};
