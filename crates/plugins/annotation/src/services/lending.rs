//! `services/lending.ts`: a sibling plugin's records shown and edited as
//! annotations (the form plugin lends its widgets in design mode). This
//! holds the lender and says which records are lent: the view shows them,
//! the store writes their edits as the lender's ops.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use epdf_kernel::error::plugin_error;
use epdf_kernel::{Error, Unsubscribe};
use serde_json::Value;

use crate::engine::anchor_of;
use crate::host_contract::AnnotationLender;
use crate::js::{key_of, key_or_none};

/// The lent records now: as the engine confirmed them, and as they show.
/// Each list keeps its identity until it changes, and so does each row: the
/// view caches a lent record per row (`lent.get(shown)` on the object), so a
/// lender that hands back the same `Rc` rows is not asked about them again.
#[derive(Clone)]
pub struct LentRecords {
    pub confirmed: Rc<Vec<Rc<Value>>>,
    pub shown: Rc<Vec<Rc<Value>>>,
}

impl LentRecords {
    pub fn same(&self, other: &LentRecords) -> bool {
        Rc::ptr_eq(&self.confirmed, &other.confirmed) && Rc::ptr_eq(&self.shown, &other.shown)
    }
}

#[derive(Default)]
pub struct Lending {
    current: RefCell<Option<Rc<dyn AnnotationLender>>>,
    /// The lent keys, per shown list.
    lent: RefCell<Option<(Rc<Vec<Rc<Value>>>, Rc<HashSet<String>>)>>,
}

impl Lending {
    pub fn lender(&self) -> Option<Rc<dyn AnnotationLender>> {
        self.current.borrow().clone()
    }

    pub fn records(&self) -> Option<LentRecords> {
        let lender = self.lender()?;
        lender.records()
    }

    fn lent_keys(&self) -> Rc<HashSet<String>> {
        let Some(records) = self.records() else { return Rc::new(HashSet::new()) };
        if let Some((list, keys)) = &*self.lent.borrow()
            && Rc::ptr_eq(list, &records.shown)
        {
            return keys.clone();
        }
        let keys: HashSet<String> =
            records.shown.iter().filter_map(|r| r.get("ref").and_then(key_of)).collect();
        let keys = Rc::new(keys);
        *self.lent.borrow_mut() = Some((records.shown.clone(), keys.clone()));
        keys
    }

    /// Whether the record is lent.
    pub fn owns(&self, id: &str) -> bool {
        self.lender().is_some() && self.lent_keys().contains(id)
    }

    /// `lend(lender)`: one lender at a time; returns the release.
    pub fn lend(
        self: &Rc<Self>,
        lender: Rc<dyn AnnotationLender>,
        notify: Rc<dyn Fn()>,
    ) -> Unsubscribe {
        *self.current.borrow_mut() = Some(lender.clone());
        notify();
        let this = Rc::downgrade(self);
        Unsubscribe::new(move || {
            let Some(this) = this.upgrade() else { return };
            let current = this.current.borrow().clone();
            if !current.is_some_and(|c| Rc::ptr_eq(&c, &lender)) {
                return;
            }
            *this.current.borrow_mut() = None;
            notify();
        })
    }

    /// `refuseLent(lending, ref)`: a lent record's change stated in code is refused.
    pub fn refuse_lent(&self, reference: &Value) -> Result<(), Error> {
        let key = key_or_none(reference);
        if !self.owns(&key) {
            return Ok(());
        }
        let owner = self.lender().map_or_else(|| "its own".to_string(), |l| l.owner());
        Err(plugin_error(
            "invalid-input",
            "annotation",
            format!("{key} belongs to the {owner} plugin: change it there"),
            None,
            None,
        ))
    }

    /// `ownReorder(lending, change, lent)`: a reorder split by owner. Returns
    /// the refs left for this plugin (`None` when none are), pushing the
    /// lender's ops onto `lent`.
    pub fn own_reorder(
        &self,
        page: &Value,
        refs: &[Value],
        position: &Value,
        lent: &mut Vec<Value>,
    ) -> Result<Option<Vec<Value>>, Error> {
        let Some(lender) = self.lender() else { return Ok(Some(refs.to_vec())) };
        let is_lent = |r: &Value| self.owns(&key_or_none(r));
        let lent_refs: Vec<Value> = refs.iter().filter(|r| is_lent(r)).cloned().collect();
        if lent_refs.is_empty() {
            return Ok(Some(refs.to_vec()));
        }
        let own_refs: Vec<Value> = refs.iter().filter(|r| !is_lent(r)).cloned().collect();
        if let Some(anchor) = anchor_of(position)
            && (!own_refs.is_empty() || !is_lent(anchor))
        {
            return Err(plugin_error(
                "invalid-input",
                "annotation",
                format!(
                    "{} records move among their own: {} can't be their neighbour",
                    lender.owner(),
                    key_or_none(anchor)
                ),
                None,
                None,
            ));
        }
        lent.extend(lender.reorder_ops(page, &lent_refs, position));
        Ok(if own_refs.is_empty() { None } else { Some(own_refs) })
    }
}

impl crate::controller::Core {
    /// `lend(lender)` (host lens): a sibling plugin's records shown and
    /// edited as annotations, one lender at a time; returns the release.
    pub fn lend(&self, lender: Rc<dyn AnnotationLender>) -> Unsubscribe {
        let cx = self.cx.clone();
        self.lending.lend(lender, Rc::new(move || cx.notify()))
    }
}
