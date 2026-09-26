//! `token.ts`: the package token. Additional lenses mint their own (see
//! [`crate::StagePluginOptions`]).

use std::sync::LazyLock;

use epdf_kernel::Token;

use crate::controller::StageCapability;

/// `StageToken`, the main lens's token, `'stage'`. The TypeScript's host
/// token is the same runtime token typed wider; here there is one
/// capability type.
pub static STAGE: LazyLock<Token<StageCapability>> = LazyLock::new(|| Token::new("stage"));
