//! `services/authority.ts`: session authority and the mode and signer
//! resolution.

use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::rt::Promise;
use epdf_signature_core::SignerPort;

use crate::contract::{SignatureMode, SignerSource};
use crate::controller::SignatureCapability;
use crate::promise::adopt;

impl SignatureCapability {
    /// `allows(cap)`: `ctx.doc?.security.allows(cap) ?? false`.
    pub(crate) fn allows(&self, capability: &str) -> bool {
        self.cx().doc().map(|doc| doc.security().allows(capability)).unwrap_or(false)
    }

    /// `getMode()`: the configured mode; `sign` with a signer, else
    /// `visual`.
    pub fn get_mode(&self) -> SignatureMode {
        let config = self.config();
        config.mode.unwrap_or(if self.signer_available() {
            SignatureMode::Sign
        } else {
            SignatureMode::Visual
        })
    }

    /// `resolveSigner(override)`: an `async` function.
    pub(crate) fn resolve_signer(&self, override_: Option<SignerSource>) -> Promise<SignerPort> {
        let signer = override_.or_else(|| self.config().signer.clone());
        Promise::spawn(async move {
            match signer {
                None => Err(plugin_error(
                    code::INVALID_INPUT,
                    "signature",
                    "no signer: configure `signer` or pass one with the call (mode \"sign\" needs one)",
                    None,
                    None,
                )),
                Some(SignerSource::Port(port)) => Ok(port),
                Some(SignerSource::Thunk(thunk)) => adopt(thunk()).await,
                Some(SignerSource::Host { resolve, .. }) => adopt(resolve()).await,
            }
        })
    }

    /// `canSign()`: `doc.sign` is granted and a signer is configured.
    pub fn can_sign(&self) -> bool {
        self.allows("doc.sign") && self.signer_available()
    }

    /// `config.signer` is set (and, for a host slot, holds a signer now).
    fn signer_available(&self) -> bool {
        self.config().signer.as_ref().is_some_and(SignerSource::is_available)
    }

    /// `canFill()`: visual fills ride `doc.forms.fill` and an engine that
    /// draws into a signature field.
    pub fn can_fill(&self) -> bool {
        self.allows("doc.forms.fill")
            && self.cx().doc().is_ok_and(|doc| doc.has("forms.setSignatureAppearance"))
    }

    /// `canCertify()`: `allowCertify` and `doc.sign.certify`.
    pub fn can_certify(&self) -> bool {
        self.config().allow_certify == Some(true) && self.allows("doc.sign.certify")
    }
}
