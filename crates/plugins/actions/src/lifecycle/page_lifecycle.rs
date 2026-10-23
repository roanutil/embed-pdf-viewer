//! `lifecycle/page-lifecycle.ts`: the coordinator's page half (the
//! document-open barrier and the cascade budget). The stage owns page truth
//! and reports it through `report_page_state`; this owns when page-lifecycle
//! triggers fire: nothing emits before the open sequence has run (or been
//! declared off or headless), and emission is a diff against the
//! last-emitted state, so pre-open motion collapses to one open.

use epdf_acrojs::js::number_to_string as num;
use epdf_kernel::geometry::PageRef;

use crate::ActionsCapability;
use crate::contract::{
    ActionDiagnostic, ActionDiagnosticCode, ActionTrigger, OpenSequence, PageEventKind,
};
use crate::host_contract::{PageStateCause, PageStateReport};
use crate::services::DiagnosticPlace;

const CASCADE_CAP: f64 = 8.0;

#[derive(Default)]
pub(crate) struct LastEmitted {
    pub(crate) current_pon: Option<u32>,
    pub(crate) visible: Vec<u32>,
}

impl ActionsCapability {
    fn page_trigger(&self, event: PageEventKind, pon: u32) {
        let _ = self.dispatch(ActionTrigger::Page { event, page: PageRef::new(pon) }, None);
    }

    fn emit_for_report(&self, report: &PageStateReport) {
        if report.cause == PageStateCause::User {
            self.0.cascade_rounds.set(0.0);
        }
        let current = report.current_page.map(|p| p.page_object_number);
        let mut next_visible: Vec<u32> = Vec::new();
        for page in &report.visible_pages {
            if !next_visible.contains(&page.page_object_number) {
                next_visible.push(page.page_object_number);
            }
        }
        let (changed_current, leaving, entering, previous_current) = {
            let last = self.0.last_emitted.borrow();
            let leaving: Vec<u32> =
                last.visible.iter().copied().filter(|p| !next_visible.contains(p)).collect();
            let entering: Vec<u32> =
                next_visible.iter().copied().filter(|p| !last.visible.contains(p)).collect();
            (current != last.current_pon, leaving, entering, last.current_pon)
        };
        if !changed_current && leaving.is_empty() && entering.is_empty() {
            return;
        }
        // Track truth even when suppressed: the budget bounds emission.
        *self.0.last_emitted.borrow_mut() =
            LastEmitted { current_pon: current, visible: next_visible };
        if report.cause == PageStateCause::Programmatic {
            self.0.cascade_rounds.set(self.0.cascade_rounds.get() + 1.0);
            if self.0.cascade_rounds.get() > CASCADE_CAP {
                self.report_diagnostic(&ActionDiagnostic::new(
                    ActionDiagnosticCode::CascadeBudget,
                    format!(
                        "page-lifecycle emission suppressed: {} consecutive programmatic rounds (cap {})",
                        num(self.0.cascade_rounds.get()),
                        num(CASCADE_CAP)
                    ),
                ), &DiagnosticPlace::default());
                return;
            }
        }
        // close(old), invisible set, visible set, open(new).
        if changed_current && let Some(previous) = previous_current {
            self.page_trigger(PageEventKind::Close, previous);
        }
        for pon in leaving {
            self.page_trigger(PageEventKind::Invisible, pon);
        }
        for pon in entering {
            self.page_trigger(PageEventKind::Visible, pon);
        }
        if changed_current && let Some(current) = current {
            self.page_trigger(PageEventKind::Open, current);
        }
    }

    /// Host lens: the stage's page-truth push door.
    pub fn report_page_state(&self, report: PageStateReport) {
        if !report.placed {
            return;
        }
        if report.cause == PageStateCause::User {
            self.0.cascade_rounds.set(0.0);
        }
        if !self.0.barrier_open.get() {
            // Coalesce: only the latest pre-open state matters.
            *self.0.buffered_report.borrow_mut() = Some(report);
            return;
        }
        self.emit_for_report(&report);
    }

    /// `releaseBarrier(fireFallback)`. The TypeScript wraps the body in a
    /// `try` whose `catch` nothing here can reach.
    pub(crate) fn release_barrier(&self, fire_fallback: bool) {
        self.0.barrier_open.set(true);
        let report = self.0.buffered_report.borrow_mut().take();
        if let Some(report) = report {
            self.emit_for_report(&report);
        } else if fire_fallback
            && self.settings_now().open_sequence == OpenSequence::Headless
            && let Some(first) = self.first_pon()
        {
            // The first-page fallback only in declared-headless mode.
            self.0.last_emitted.borrow_mut().current_pon = Some(first);
            self.page_trigger(PageEventKind::Open, first);
        }
    }

    /// A user gesture resets the programmatic-cascade counter.
    pub(crate) fn reset_cascade(&self) {
        self.0.cascade_rounds.set(0.0);
    }
}
