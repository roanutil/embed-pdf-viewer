//! The second half of pkijs's `CertificateChainValidationEngine.verify()`:
//! certificate policies and name constraints over the sorted path, with
//! the defaults `validateChain` passes (any policy, nothing explicit, no
//! initial subtrees). A path whose certificates carry none of these
//! extensions passes untouched, which is every path the vectors build.
//!
//! The TypeScript works on sparse arrays and `undefined`; this port keeps
//! the same observable results, including the `TypeError` text a malformed
//! mapping produces.

// The loops index several parallel arrays at once, as the TypeScript does.
#![allow(clippy::needless_range_loop)]

use crate::cert::{Certificate, Name, string_prep};
use crate::der::{self, Node};

const ANY_POLICY: &str = "2.5.29.32.0";
const CERTIFICATE_POLICIES: &str = "2.5.29.32";
const POLICY_MAPPINGS: &str = "2.5.29.33";
const POLICY_CONSTRAINTS: &str = "2.5.29.36";
const INHIBIT_ANY_POLICY: &str = "2.5.29.54";
const NAME_CONSTRAINTS: &str = "2.5.29.30";
const SUBJECT_ALT_NAME: &str = "2.5.29.17";
const EMAIL: &str = "1.2.840.113549.1.9.1";
const RFC822_MAILBOX: &str = "0.9.2342.19200300.100.1.3";

fn type_error(member: &str) -> String {
    format!("Cannot read properties of undefined (reading '{member}')")
}

fn integer(node: &Node) -> f64 {
    node.content.iter().fold(0.0, |acc, &b| acc * 256.0 + f64::from(b))
}

/// `CertificatePolicies`: the policy identifiers, in order.
fn policies(node: &Node) -> Vec<String> {
    node.children.iter().filter_map(|info| info.children.first().and_then(Node::oid)).collect()
}

/// `PolicyMappings`: `(issuerDomainPolicy, subjectDomainPolicy)` pairs.
fn mappings(node: &Node) -> Vec<(String, String)> {
    node.children
        .iter()
        .filter_map(|m| Some((m.children.first()?.oid()?, m.children.get(1)?.oid()?)))
        .collect()
}

/// `PolicyConstraints`: each member present only when the input has it,
/// `NaN` standing in for `undefined` in the arithmetic.
fn constraints(node: &Node) -> (f64, f64) {
    let mut require = f64::NAN;
    let mut inhibit = f64::NAN;
    for member in &node.children {
        match (member.class, member.tag) {
            (der::CONTEXT, 0) => require = integer(member),
            (der::CONTEXT, 1) => inhibit = integer(member),
            _ => {}
        }
    }
    (require, inhibit)
}

#[derive(Clone, Debug)]
enum GeneralName {
    Text(u32, String),
    Ip(Vec<u8>),
    Directory(Name),
    Other(u32),
}

impl GeneralName {
    fn read(node: &Node) -> Option<GeneralName> {
        if node.class != der::CONTEXT {
            return None;
        }
        Some(match node.tag {
            1 | 2 | 6 => {
                GeneralName::Text(node.tag, node.content.iter().map(|&b| b as char).collect())
            }
            7 => GeneralName::Ip(node.content.clone()),
            4 => GeneralName::Directory(Name::read(node.children.first()?)?),
            other => GeneralName::Other(other),
        })
    }

    fn kind(&self) -> u32 {
        match self {
            GeneralName::Text(t, _) => *t,
            GeneralName::Ip(_) => 7,
            GeneralName::Directory(_) => 4,
            GeneralName::Other(t) => *t,
        }
    }
}

fn subtrees(node: &Node) -> Vec<GeneralName> {
    node.children.iter().filter_map(|s| s.children.first().and_then(GeneralName::read)).collect()
}

fn compare_dns_name(name: &str, constraint: &str) -> bool {
    let name = string_prep(name);
    let constraint = string_prep(constraint);
    let ns: Vec<&str> = name.split('.').collect();
    let cs: Vec<&str> = constraint.split('.').collect();
    if ns.len() < cs.len() || ns.iter().any(|p| p.is_empty()) {
        return false;
    }
    for (i, part) in cs.iter().enumerate() {
        if part.is_empty() {
            if i == 0 {
                if cs.len() == 1 {
                    return false;
                }
                continue;
            }
            return false;
        }
    }
    (0..cs.len())
        .all(|i| cs[cs.len() - 1 - i].is_empty() || ns[ns.len() - 1 - i] == cs[cs.len() - 1 - i])
}

fn compare_rfc822_name(name: &str, constraint: &str) -> Result<bool, String> {
    let prepared = string_prep(name);
    let constraint_prepared = string_prep(constraint);
    let ns: Vec<&str> = prepared.split('@').collect();
    let cs: Vec<&str> = constraint_prepared.split('@').collect();
    if ns.len() < cs.len() {
        return Ok(false);
    }
    if cs.len() == 1 {
        let host = ns.get(1).ok_or_else(|| type_error("trim"))?;
        if compare_dns_name(host, cs[0]) {
            let hs = host.split('.').count();
            let first_empty = cs[0].split('.').next().is_some_and(str::is_empty);
            return Ok(first_empty || hs == cs[0].split('.').count());
        }
        return Ok(false);
    }
    Ok(prepared == constraint_prepared)
}

fn compare_uri(name: &str, constraint: &str) -> bool {
    let mut prepared = string_prep(name);
    let constraint = string_prep(constraint);
    if constraint.split('/').count() > 1 {
        return false;
    }
    let ns: Vec<String> = prepared.split('/').map(str::to_string).collect();
    if ns.len() > 1
        && let Some(part) = ns.iter().find(|p| !p.is_empty() && !p.ends_with(':'))
    {
        prepared = part.split(':').next().unwrap_or_default().to_string();
    }
    if compare_dns_name(&prepared, &constraint) {
        let first_empty = constraint.split('.').next().is_some_and(str::is_empty);
        return first_empty || prepared.split('.').count() == constraint.split('.').count();
    }
    false
}

fn compare_ip(name: &[u8], constraint: &[u8]) -> bool {
    match (name.len(), constraint.len()) {
        (4, 8) => (0..4).all(|i| (name[i] ^ constraint[i]) & constraint[i + 4] == 0),
        (16, 32) => (0..16).all(|i| (name[i] ^ constraint[i]) & constraint[i + 16] == 0),
        _ => false,
    }
}

fn attribute_equal(a: &(String, Node), b: &(String, Node)) -> bool {
    let one = Name { raw: Vec::new(), attributes: vec![a.clone()] };
    let two = Name { raw: Vec::new(), attributes: vec![b.clone()] };
    one.is_equal(&two)
}

fn compare_directory_name(name: &Name, constraint: &Name) -> bool {
    if name.attributes.is_empty() || constraint.attributes.is_empty() {
        return true;
    }
    if name.attributes.len() < constraint.attributes.len() {
        return false;
    }
    let mut result = true;
    let mut start = 0;
    for c in &constraint.attributes {
        let mut local = false;
        for j in start..name.attributes.len() {
            local = attribute_equal(&name.attributes[j], c);
            if name.attributes[j].0 == c.0 {
                result = result && local;
            }
            if local {
                if start == 0 || start == j {
                    start = j + 1;
                    break;
                }
                return false;
            }
        }
        if !local {
            return false;
        }
    }
    start != 0 && result
}

/// The policy and name-constraint checks: `Err` with pkijs's message.
pub fn check(path: &[&Certificate]) -> Result<(), String> {
    let n = path.len();
    if n < 2 {
        return Ok(());
    }
    let slots = n - 1;
    let mut explicit = false;
    let mut mapping_inhibit = false;
    let mut inhibit_any = false;
    let mut pending = [false; 3];
    let (mut explicit_pending, mut mapping_pending, mut any_pending) = (0.0_f64, 0.0_f64, 0.0_f64);
    let mut all_policies = vec![ANY_POLICY.to_string()];
    let mut per_cert: Vec<Vec<Option<bool>>> = vec![vec![Some(true); slots]];
    let mut policy_mappings: Vec<Option<Vec<(String, String)>>> = vec![None; slots];
    let mut cert_policies: Vec<Option<Vec<String>>> = vec![None; slots];
    let mut explicit_start: i64 = -1;

    for i in (0..slots).rev() {
        let Some(extensions) = &path[i].extensions else { continue };
        for extension in extensions {
            let parsed = extension.parsed();
            let node = parsed.clone().unwrap_or(Node {
                class: 0,
                constructed: true,
                tag: 16,
                raw: Vec::new(),
                content: Vec::new(),
                children: Vec::new(),
            });
            match extension.oid.as_str() {
                CERTIFICATE_POLICIES => {
                    let list = policies(&node);
                    cert_policies[i] = Some(list.clone());
                    if let Some(s) = all_policies.iter().position(|p| p == ANY_POLICY) {
                        per_cert[s][i] = None;
                    }
                    for policy in list {
                        match all_policies.iter().position(|p| *p == policy) {
                            Some(index) => per_cert[index][i] = Some(true),
                            None => {
                                all_policies.push(policy);
                                let mut slot = vec![None; slots];
                                slot[i] = Some(true);
                                per_cert.push(slot);
                            }
                        }
                    }
                }
                POLICY_MAPPINGS => {
                    if mapping_inhibit {
                        return Err("Policy mapping prohibited".into());
                    }
                    policy_mappings[i] = Some(mappings(&node));
                }
                POLICY_CONSTRAINTS if !explicit => {
                    let (require, inhibit) = constraints(&node);
                    if require == 0.0 {
                        explicit = true;
                        explicit_start = i as i64;
                    } else if !pending[0] {
                        pending[0] = true;
                        explicit_pending = require;
                    } else if explicit_pending > require {
                        explicit_pending = require;
                    }
                    if inhibit == 0.0 {
                        mapping_inhibit = true;
                    } else if !pending[1] {
                        pending[1] = true;
                        mapping_pending = inhibit + 1.0;
                    } else if mapping_pending > inhibit + 1.0 {
                        mapping_pending = inhibit + 1.0;
                    }
                }
                INHIBIT_ANY_POLICY if !inhibit_any => {
                    let value =
                        if node.is_universal(der::INTEGER) { integer(&node) } else { f64::NAN };
                    if value == 0.0 {
                        inhibit_any = true;
                    } else if !pending[2] {
                        pending[2] = true;
                        any_pending = value;
                    } else if any_pending > value {
                        any_pending = value;
                    }
                }
                _ => {}
            }
        }
        if inhibit_any && all_policies.iter().any(|p| p == ANY_POLICY) {
            per_cert[0][i] = None;
        }
        if !explicit && pending[0] {
            explicit_pending -= 1.0;
            if explicit_pending == 0.0 {
                explicit = true;
                explicit_start = i as i64;
                pending[0] = false;
            }
        }
        if !mapping_inhibit && pending[1] {
            mapping_pending -= 1.0;
            if mapping_pending == 0.0 {
                mapping_inhibit = true;
                pending[1] = false;
            }
        }
        if !inhibit_any && pending[2] {
            any_pending -= 1.0;
            if any_pending == 0.0 {
                inhibit_any = true;
                pending[2] = false;
            }
        }
    }

    for i in 0..slots {
        let Some(maps) = (i + 1 < slots).then(|| policy_mappings[i + 1].clone()).flatten() else {
            continue;
        };
        for (issuer_domain, subject_domain) in maps {
            if issuer_domain == ANY_POLICY || subject_domain == ANY_POLICY {
                return Err(
                    "The \"anyPolicy\" should not be a part of policy mapping scheme".into()
                );
            }
            let issuer_index = all_policies.iter().rposition(|p| *p == issuer_domain);
            let subject_index = all_policies.iter().rposition(|p| *p == subject_domain);
            let issuer_slot = issuer_index.ok_or_else(|| type_error(&i.to_string()))?;
            per_cert[issuer_slot][i] = None;
            let listed =
                cert_policies[i].as_ref().ok_or_else(|| type_error("certificatePolicies"))?;
            for policy in listed {
                if *policy == subject_domain
                    && let Some(subject_slot) = subject_index
                {
                    for m in 0..=i {
                        if per_cert[subject_slot][m].is_some() {
                            per_cert[issuer_slot][m] = Some(true);
                            per_cert[subject_slot][m] = None;
                        }
                    }
                }
            }
        }
    }

    for (index, policy) in all_policies.iter().enumerate() {
        if policy == ANY_POLICY {
            for j in 0..explicit_start.max(0) as usize {
                if j < slots {
                    per_cert[index][j] = None;
                }
            }
        }
    }

    let any_index = all_policies.iter().position(|p| p == ANY_POLICY);
    let mut auth: Vec<String> = Vec::new();
    for (index, policy) in all_policies.iter().enumerate() {
        let mut found = true;
        for j in 0..slots {
            if (j as i64) < explicit_start && policy == ANY_POLICY && all_policies.len() > 1 {
                found = false;
                break;
            }
            if per_cert[index][j].is_none() {
                let any_found = (j as i64) >= explicit_start
                    && any_index.is_some_and(|a| per_cert[a][j] == Some(true));
                if !any_found {
                    found = false;
                    break;
                }
            }
        }
        if found {
            auth.push(policy.clone());
        }
    }
    let user_policies = if !explicit || (auth.len() == 1 && auth[0] == ANY_POLICY) {
        vec![ANY_POLICY.to_string()]
    } else {
        auth.clone()
    };
    if user_policies.is_empty() {
        return Err(
            "Zero \"userConstrPolicies\" array, no intersections with \"authConstrPolicies\""
                .into(),
        );
    }

    let mut permitted: Vec<GeneralName> = Vec::new();
    let mut excluded: Vec<GeneralName> = Vec::new();
    for i in (0..slots).rev() {
        let cert = path[i];
        let mut alt_names: Vec<GeneralName> = Vec::new();
        let mut cert_permitted = Vec::new();
        let mut cert_excluded = Vec::new();
        for extension in cert.extensions.iter().flatten() {
            let Some(node) = extension.parsed() else { continue };
            if extension.oid == NAME_CONSTRAINTS {
                for member in &node.children {
                    match (member.class, member.tag) {
                        (der::CONTEXT, 0) => cert_permitted.extend(subtrees(member)),
                        (der::CONTEXT, 1) => cert_excluded.extend(subtrees(member)),
                        _ => {}
                    }
                }
            }
            if extension.oid == SUBJECT_ALT_NAME {
                alt_names.extend(node.children.iter().filter_map(GeneralName::read));
            }
        }
        let emails: Vec<String> = cert
            .subject
            .attributes
            .iter()
            .filter(|(t, _)| t == EMAIL || t == RFC822_MAILBOX)
            .map(|(_, v)| v.text().unwrap_or_default())
            .collect();
        for kind in [1u32, 2, 4, 6, 7] {
            let group: Vec<&GeneralName> = permitted.iter().filter(|g| g.kind() == kind).collect();
            let mut group_permitted = false;
            let mut value_exists = false;
            for constraint in &group {
                match (kind, constraint) {
                    (1, GeneralName::Text(_, c)) => {
                        if alt_names.is_empty() {
                            for email in &emails {
                                value_exists = true;
                                group_permitted = group_permitted || compare_rfc822_name(email, c)?;
                            }
                        } else {
                            for alt in &alt_names {
                                if let GeneralName::Text(1, v) = alt {
                                    value_exists = true;
                                    group_permitted = group_permitted || compare_rfc822_name(v, c)?;
                                }
                            }
                        }
                    }
                    (2 | 6, GeneralName::Text(_, c)) => {
                        for alt in &alt_names {
                            if let GeneralName::Text(t, v) = alt
                                && *t == kind
                            {
                                value_exists = true;
                                let hit = if kind == 2 {
                                    compare_dns_name(v, c)
                                } else {
                                    compare_uri(v, c)
                                };
                                group_permitted = group_permitted || hit;
                            }
                        }
                    }
                    (4, GeneralName::Directory(c)) => {
                        value_exists = true;
                        group_permitted = compare_directory_name(&cert.subject, c);
                    }
                    (7, GeneralName::Ip(c)) => {
                        for alt in &alt_names {
                            if let GeneralName::Ip(v) = alt {
                                value_exists = true;
                                group_permitted = group_permitted || compare_ip(v, c);
                            }
                        }
                    }
                    _ => {}
                }
                if group_permitted {
                    break;
                }
            }
            if !group_permitted && !group.is_empty() && value_exists {
                return Err("Failed to meet \"permitted sub-trees\" name constraint".into());
            }
        }
        let mut is_excluded = false;
        for constraint in &excluded {
            match constraint {
                GeneralName::Text(1, c) => {
                    for alt in &alt_names {
                        if let GeneralName::Text(1, v) = alt {
                            is_excluded = is_excluded || compare_rfc822_name(v, c)?;
                        }
                    }
                }
                GeneralName::Text(kind @ (2 | 6), c) => {
                    for alt in &alt_names {
                        if let GeneralName::Text(t, v) = alt
                            && t == kind
                        {
                            let hit =
                                if *kind == 2 { compare_dns_name(v, c) } else { compare_uri(v, c) };
                            is_excluded = is_excluded || hit;
                        }
                    }
                }
                GeneralName::Directory(c) => {
                    is_excluded = is_excluded || compare_directory_name(&cert.subject, c)
                }
                GeneralName::Ip(c) => {
                    for alt in &alt_names {
                        if let GeneralName::Ip(v) = alt {
                            is_excluded = is_excluded || compare_ip(v, c);
                        }
                    }
                }
                _ => {}
            }
            if is_excluded {
                break;
            }
        }
        if is_excluded {
            return Err("Failed to meet \"excluded sub-trees\" name constraint".into());
        }
        permitted.extend(cert_permitted);
        excluded.extend(cert_excluded);
    }
    Ok(())
}
