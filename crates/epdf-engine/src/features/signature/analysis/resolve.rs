//! `packages/engine/core/src/signature/analysis/resolve.ts`: inbound edges
//! resolved to anchors. The reasoning is in the TypeScript.

use std::collections::{HashMap, HashSet};

use super::types::ObjectReferrer;

/// `EdgeResolverBudget`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EdgeResolverBudget {
    pub max_depth: usize,
    pub max_edges_per_object: usize,
    pub max_reads: usize,
}

/// `DEFAULT_EDGE_RESOLVER_BUDGET`.
pub const DEFAULT_EDGE_RESOLVER_BUDGET: EdgeResolverBudget =
    EdgeResolverBudget { max_depth: 16, max_edges_per_object: 256, max_reads: 20_000 };

#[derive(Debug, Clone)]
struct Resolution {
    edges: Vec<ObjectReferrer>,
    cyclic: bool,
}

/// `EdgeResolver`. `resolve` answers `None` for `USAGE_INCOMPLETE`.
pub struct EdgeResolver<F: FnMut(u32) -> Vec<ObjectReferrer>> {
    read: F,
    anchors: HashSet<u32>,
    budget: EdgeResolverBudget,
    memo: HashMap<u32, Resolution>,
    reads: usize,
    exhausted: bool,
}

impl<F: FnMut(u32) -> Vec<ObjectReferrer>> EdgeResolver<F> {
    pub fn new(read: F, anchors: HashSet<u32>) -> Self {
        Self::with_budget(read, anchors, DEFAULT_EDGE_RESOLVER_BUDGET)
    }

    pub fn with_budget(read: F, anchors: HashSet<u32>, budget: EdgeResolverBudget) -> Self {
        EdgeResolver { read, anchors, budget, memo: HashMap::new(), reads: 0, exhausted: false }
    }

    /// Anchored edges of `object_number`, or `None` when a budget was
    /// exceeded.
    pub fn resolve(&mut self, object_number: u32) -> Option<Vec<ObjectReferrer>> {
        if self.exhausted {
            return None;
        }
        let mut active = Vec::new();
        self.resolve_inner(object_number, &mut active).map(|r| r.edges)
    }

    fn resolve_inner(&mut self, n: u32, active: &mut Vec<u32>) -> Option<Resolution> {
        if let Some(hit) = self.memo.get(&n) {
            return Some(hit.clone());
        }
        if active.len() >= self.budget.max_depth {
            return None;
        }
        if self.reads >= self.budget.max_reads {
            self.exhausted = true;
            return None;
        }
        self.reads += 1;
        let raw = (self.read)(n);

        active.push(n);
        let result = self.walk(raw, active);
        active.retain(|a| *a != n);
        let resolution = result?;
        if !resolution.cyclic {
            self.memo.insert(n, resolution.clone());
        }
        Some(resolution)
    }

    fn walk(&mut self, raw: Vec<ObjectReferrer>, active: &mut Vec<u32>) -> Option<Resolution> {
        let mut edges = Vec::new();
        let mut cyclic = false;
        for e in raw {
            if self.anchors.contains(&e.parent) {
                edges.push(e);
            } else if active.contains(&e.parent) {
                cyclic = true; // a loop back into the chain: no new anchor lies that way
                continue;
            } else {
                let up = self.resolve_inner(e.parent, active)?;
                cyclic |= up.cyclic;
                for u in up.edges {
                    let mut via = u.via.unwrap_or_default();
                    via.push(e.parent);
                    edges.push(ObjectReferrer {
                        parent: u.parent,
                        label: format!("{}/{}", u.label, e.label),
                        via: Some(via),
                    });
                }
            }
            if edges.len() > self.budget.max_edges_per_object {
                return None;
            }
        }
        Some(Resolution { edges, cyclic })
    }
}
