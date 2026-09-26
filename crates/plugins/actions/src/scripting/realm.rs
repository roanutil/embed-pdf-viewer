//! `scripting/realm.ts`: the one per-document script host. Policy decides
//! whether a realm factory exists; the factory mints this document's realm
//! and, on demand, detached ones: one environment, any number of isolated
//! realms.

use crate::ActionsCapability;
use crate::host_contract::{DetachedScriptRealm, ScriptRealmPort, ScriptRealmTarget};

impl ActionsCapability {
    /// Did `javascript.enabled` take effect: a script realm exists for this
    /// document.
    pub fn is_scripting_enabled(&self) -> bool {
        self.0.script_host.is_some()
    }

    /// Host lens: this document's realm, present only with scripting on (its
    /// presence is the form plugin's "scripting on" signal).
    pub fn script_realm(&self) -> Option<ScriptRealmPort> {
        let host = self.0.script_host.clone()?;
        let realms = self.0.realms.as_ref()?;
        Some(ScriptRealmPort { host, budget: realms.budget.clone() })
    }

    /// Host lens: mint a realm for a detached document under this document's
    /// policy and environment (the same sandbox factory, identity, clock and
    /// budget; a fresh sandbox). `None` with scripting off, where the
    /// TypeScript member is absent. The caller owns the realm and surfaces its
    /// results with `realm: 'detached'`.
    pub fn create_detached_script_realm(
        &self,
        target: ScriptRealmTarget,
    ) -> Option<DetachedScriptRealm> {
        self.0.script_host.as_ref()?;
        let realms = self.0.realms.as_ref()?;
        let host = realms.realm_for(target);
        Some(DetachedScriptRealm { port: ScriptRealmPort { host, budget: realms.budget.clone() } })
    }
}
