//! `token.ts`: the package token.

use std::sync::LazyLock;

use epdf_kernel::Token;

use crate::controller::SearchCapability;

/// `SearchToken`, `'search'`.
pub static SEARCH: LazyLock<Token<SearchCapability>> = LazyLock::new(|| Token::new("search"));
