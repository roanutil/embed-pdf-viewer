//! `packages/engine/core/src/signature/analysis/verdict.ts`: one
//! aggregation rule everywhere.

use super::types::{Assessment, ChangeFinding, FindingVerdict, RevisionAnalysis, StepVerdict};

fn rank(v: StepVerdict) -> u8 {
    match v {
        StepVerdict::Unchanged => 0,
        StepVerdict::Permitted => 1,
        StepVerdict::Indeterminate => 2,
        StepVerdict::Forbidden => 3,
    }
}

/// `worstVerdict`.
pub fn worst_verdict(steps: &[RevisionAnalysis]) -> StepVerdict {
    let mut worst = StepVerdict::Unchanged;
    for s in steps {
        if rank(s.verdict) > rank(worst) {
            worst = s.verdict;
        }
    }
    worst
}

/// `conclude`.
pub fn conclude(findings: &[ChangeFinding], has_effective_changes: bool) -> StepVerdict {
    if findings.iter().any(|f| f.verdict == FindingVerdict::Forbidden) {
        return StepVerdict::Forbidden;
    }
    if findings.iter().any(|f| f.verdict == FindingVerdict::Incomplete) {
        return StepVerdict::Indeterminate;
    }
    if has_effective_changes { StepVerdict::Permitted } else { StepVerdict::Unchanged }
}

/// `primaryFinding`.
pub fn primary_finding(findings: &[ChangeFinding]) -> Option<ChangeFinding> {
    [FindingVerdict::Forbidden, FindingVerdict::Incomplete, FindingVerdict::Permitted]
        .into_iter()
        .find_map(|v| findings.iter().find(|f| f.verdict == v))
        .cloned()
}

/// `assessmentOf`.
pub fn assessment_of(step: &RevisionAnalysis) -> Assessment {
    Assessment {
        verdict: step.verdict,
        complete: !step.findings.iter().any(|f| f.verdict == FindingVerdict::Incomplete),
        primary: primary_finding(&step.findings),
        findings: step.findings.clone(),
    }
}

/// `combine`. `steps` may hold `net` itself (a one-step window, as a clone
/// here), which is skipped as the TypeScript's identity check skips it: by
/// value, since no other step spans the same revisions.
pub fn combine(net: &RevisionAnalysis, steps: &[RevisionAnalysis]) -> Assessment {
    let mut findings = net.findings.clone();
    for s in steps {
        if s == net {
            continue;
        }
        for f in &s.findings {
            if f.verdict == FindingVerdict::Permitted {
                continue;
            }
            findings.push(ChangeFinding {
                detail: Some(format!(
                    "revision {}: {}",
                    s.newer,
                    f.detail.as_deref().unwrap_or(&f.rule)
                )),
                ..f.clone()
            });
        }
    }
    let verdict = conclude(
        &findings,
        net.verdict != StepVerdict::Unchanged
            || steps.iter().any(|s| s.verdict != StepVerdict::Unchanged),
    );
    Assessment {
        verdict,
        complete: !findings.iter().any(|f| f.verdict == FindingVerdict::Incomplete),
        primary: primary_finding(&findings),
        findings,
    }
}
