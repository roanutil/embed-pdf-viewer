//! Identity-bearing values for the current `state.ts` shallow comparison.
use std::rc::Rc;

#[derive(Clone, Debug)]
pub enum StateValue {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Object(Rc<Vec<(String, StateValue)>>),
    Array(Rc<Vec<StateValue>>),
}
impl StateValue {
    pub fn object(fields: Vec<(String, Self)>) -> Self {
        Self::Object(Rc::new(fields))
    }
    pub fn array(items: Vec<Self>) -> Self {
        Self::Array(Rc::new(items))
    }
    pub fn object_is(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Undefined, Self::Undefined) | (Self::Null, Self::Null) => true,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Number(a), Self::Number(b)) => {
                (a.is_nan() && b.is_nan()) || a.to_bits() == b.to_bits()
            }
            (Self::String(a), Self::String(b)) => a == b,
            (Self::Object(a), Self::Object(b)) => Rc::ptr_eq(a, b),
            (Self::Array(a), Self::Array(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}
pub fn shallow_equal(left: &StateValue, right: &StateValue) -> bool {
    if left.object_is(right) {
        return true;
    }
    match (left, right) {
        (StateValue::Object(a), StateValue::Object(b)) => {
            a.len() == b.len()
                && a.iter().all(|(key, value)| {
                    b.iter()
                        .find(|(k, _)| k == key)
                        .is_some_and(|(_, other)| value.object_is(other))
                })
        }
        (StateValue::Array(a), StateValue::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b.iter()).all(|(a, b)| a.object_is(b))
        }
        _ => false,
    }
}
