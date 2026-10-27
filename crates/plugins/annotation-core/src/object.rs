//! Typed values that print the way the TypeScript objects they mirror print.
//!
//! A TypeScript object literal fixes its members' order, a spread keeps the
//! order of what it spreads and puts new members last, and the conformance
//! comparison checks member order. So each port of an object type here
//! remembers the order its members arrived in ([`Order`]): read from JSON it
//! keeps the order it was read in, and a member set later that it didn't
//! have prints last, as a spread adds it. A value built in code prints in
//! declaration order until [`Order`] is sealed.
//!
//! The values cross JSON through [`Json`] rather than serde, so canonical
//! `NaN` and `undefined` (`json.rs`) read and print as the TypeScript's.

use std::rc::Rc;

use epdf_geometry::{Point, Quad, Rect};
use serde_json::{Map, Value};

use crate::json::{self, Object};

/// Why a JSON value doesn't read as the type asked for.
#[derive(Clone, Debug, PartialEq)]
pub struct JsonError(pub String);

impl JsonError {
    pub fn new(message: impl Into<String>) -> Self {
        JsonError(message.into())
    }

    /// The same error, under member `key`.
    pub fn at(self, key: &str) -> Self {
        JsonError(format!("{key}: {}", self.0))
    }
}

impl std::fmt::Display for JsonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A value that crosses canonical JSON. `None` is an absent member (or
/// JavaScript `undefined`).
pub trait Json: Sized {
    fn from_json(value: Option<&Value>) -> Result<Self, JsonError>;
    fn to_json(&self) -> Option<Value>;

    /// Read a present value.
    fn read(value: &Value) -> Result<Self, JsonError> {
        Self::from_json(Some(value))
    }

    /// Print a value that is always present.
    fn json(&self) -> Value {
        self.to_json().unwrap_or(Value::Null)
    }
}

fn present(value: Option<&Value>) -> Option<&Value> {
    value.filter(|v| !json::undefined(Some(v)))
}

fn missing() -> JsonError {
    JsonError::new("missing")
}

impl Json for f64 {
    fn from_json(value: Option<&Value>) -> Result<Self, JsonError> {
        let value = present(value).ok_or_else(missing)?;
        json::number(value).ok_or_else(|| JsonError::new(format!("not a number: {value}")))
    }
    fn to_json(&self) -> Option<Value> {
        Some(json::num(*self))
    }
}

impl Json for bool {
    fn from_json(value: Option<&Value>) -> Result<Self, JsonError> {
        present(value).ok_or_else(missing)?.as_bool().ok_or_else(|| JsonError::new("not a boolean"))
    }
    fn to_json(&self) -> Option<Value> {
        Some(Value::Bool(*self))
    }
}

impl Json for String {
    fn from_json(value: Option<&Value>) -> Result<Self, JsonError> {
        let value = present(value).ok_or_else(missing)?;
        value
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| JsonError::new(format!("not a string: {value}")))
    }
    fn to_json(&self) -> Option<Value> {
        Some(Value::String(self.clone()))
    }
}

impl Json for Value {
    fn from_json(value: Option<&Value>) -> Result<Self, JsonError> {
        present(value).cloned().ok_or_else(missing)
    }
    fn to_json(&self) -> Option<Value> {
        Some(self.clone())
    }
}

impl Json for Object {
    fn from_json(value: Option<&Value>) -> Result<Self, JsonError> {
        present(value)
            .ok_or_else(missing)?
            .as_object()
            .cloned()
            .ok_or_else(|| JsonError::new("not an object"))
    }
    fn to_json(&self) -> Option<Value> {
        Some(Value::Object(self.clone()))
    }
}

impl<T: Json> Json for Option<T> {
    /// Absent is `None`. `null` reads as the inner type's `null` when it has
    /// one (a raw value), else as absent (`value == null`).
    fn from_json(value: Option<&Value>) -> Result<Self, JsonError> {
        match present(value) {
            None => Ok(None),
            Some(Value::Null) => Ok(T::from_json(Some(&Value::Null)).ok()),
            Some(v) => T::from_json(Some(v)).map(Some),
        }
    }
    fn to_json(&self) -> Option<Value> {
        self.as_ref().and_then(T::to_json)
    }
}

impl<T: Json> Json for Vec<T> {
    fn from_json(value: Option<&Value>) -> Result<Self, JsonError> {
        let items = present(value)
            .ok_or_else(missing)?
            .as_array()
            .ok_or_else(|| JsonError::new("not an array"))?;
        items
            .iter()
            .enumerate()
            .map(|(i, v)| T::read(v).map_err(|e| e.at(&i.to_string())))
            .collect()
    }
    fn to_json(&self) -> Option<Value> {
        Some(Value::Array(self.iter().map(|item| item.to_json().unwrap_or(Value::Null)).collect()))
    }
}

impl<T: Json> Json for Rc<T> {
    fn from_json(value: Option<&Value>) -> Result<Self, JsonError> {
        T::from_json(value).map(Rc::new)
    }
    fn to_json(&self) -> Option<Value> {
        (**self).to_json()
    }
}

impl Json for Point {
    fn from_json(value: Option<&Value>) -> Result<Self, JsonError> {
        let object = present(value)
            .ok_or_else(missing)?
            .as_object()
            .ok_or_else(|| JsonError::new("not a point"))?;
        Ok(Point { x: f64::from_json(object.get("x"))?, y: f64::from_json(object.get("y"))? })
    }
    fn to_json(&self) -> Option<Value> {
        Some(json::point_value(*self))
    }
}

impl Json for Rect {
    fn from_json(value: Option<&Value>) -> Result<Self, JsonError> {
        let object = present(value)
            .ok_or_else(missing)?
            .as_object()
            .ok_or_else(|| JsonError::new("not a rect"))?;
        Ok(Rect {
            x: f64::from_json(object.get("x")).map_err(|e| e.at("x"))?,
            y: f64::from_json(object.get("y")).map_err(|e| e.at("y"))?,
            width: f64::from_json(object.get("width")).map_err(|e| e.at("width"))?,
            height: f64::from_json(object.get("height")).map_err(|e| e.at("height"))?,
        })
    }
    fn to_json(&self) -> Option<Value> {
        Some(json::rect_value(*self))
    }
}

impl Json for Quad {
    fn from_json(value: Option<&Value>) -> Result<Self, JsonError> {
        let object = present(value)
            .ok_or_else(missing)?
            .as_object()
            .ok_or_else(|| JsonError::new("not a quad"))?;
        Ok(Quad {
            upper_left: Point::from_json(object.get("upperLeft"))?,
            upper_right: Point::from_json(object.get("upperRight"))?,
            lower_left: Point::from_json(object.get("lowerLeft"))?,
            lower_right: Point::from_json(object.get("lowerRight"))?,
        })
    }
    fn to_json(&self) -> Option<Value> {
        Some(json::quad_value(*self))
    }
}

/// A member that is either `null` or a value (`T | null`).
#[derive(Clone, Debug, PartialEq, Default)]
pub enum Nullable<T> {
    #[default]
    Null,
    Value(T),
}

impl<T> Nullable<T> {
    pub fn as_option(&self) -> Option<&T> {
        match self {
            Nullable::Null => None,
            Nullable::Value(v) => Some(v),
        }
    }

    pub fn into_option(self) -> Option<T> {
        match self {
            Nullable::Null => None,
            Nullable::Value(v) => Some(v),
        }
    }

    pub fn from_option(value: Option<T>) -> Self {
        value.map_or(Nullable::Null, Nullable::Value)
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Nullable::Null)
    }
}

impl<T: Json> Json for Nullable<T> {
    fn from_json(value: Option<&Value>) -> Result<Self, JsonError> {
        match present(value) {
            None => Err(missing()),
            Some(Value::Null) => Ok(Nullable::Null),
            Some(v) => T::read(v).map(Nullable::Value),
        }
    }
    fn to_json(&self) -> Option<Value> {
        Some(match self {
            Nullable::Null => Value::Null,
            Nullable::Value(v) => v.to_json().unwrap_or(Value::Null),
        })
    }
}

/// The order an object's members print in: indices into its type's keys,
/// empty for declaration order.
#[derive(Clone, Debug, Default)]
pub struct Order(pub Vec<u8>);

impl PartialEq for Order {
    /// Members compare by value; their order is how they print, not what they are.
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Order {
    /// The order the members of `object` arrived in, among `keys`.
    pub fn of(object: &Map<String, Value>, keys: &[&str]) -> Order {
        Order(
            object
                .keys()
                .filter_map(|key| keys.iter().position(|k| k == key).map(|i| i as u8))
                .collect(),
        )
    }

    /// An explicit order, for an object a literal builds in another order than declared.
    pub fn named(names: &[&str], keys: &[&str]) -> Order {
        Order(
            names
                .iter()
                .filter_map(|n| keys.iter().position(|k| k == n).map(|i| i as u8))
                .collect(),
        )
    }

    /// Print `values` (one per declared key; `None` absent) in this order,
    /// members set since and not in it last, in declaration order.
    pub fn print(&self, keys: &[&str], values: Vec<Option<Value>>) -> Map<String, Value> {
        let mut out = Map::new();
        let mut done = vec![false; keys.len()];
        for &i in &self.0 {
            let i = i as usize;
            if let Some(Some(value)) = values.get(i) {
                out.insert(keys[i].into(), value.clone());
            }
            if i < done.len() {
                done[i] = true;
            }
        }
        for (i, value) in values.into_iter().enumerate() {
            if !done[i]
                && let Some(value) = value
            {
                out.insert(keys[i].into(), value);
            }
        }
        out
    }

    /// Fix the order to the members `present` now (declaration order when it
    /// was declaration order), so a member set later prints last.
    pub fn seal(&mut self, present: &[bool]) {
        let mut sealed: Vec<u8> =
            self.0.iter().copied().filter(|i| present.get(*i as usize) == Some(&true)).collect();
        for (i, p) in present.iter().enumerate() {
            if *p && !sealed.contains(&(i as u8)) {
                sealed.push(i as u8);
            }
        }
        self.0 = sealed;
    }

    /// Drop member `i` from the order, as a rest destructure drops it: set
    /// again, it prints last.
    pub fn forget(&mut self, i: usize) {
        self.0.retain(|&k| k as usize != i);
    }
}

/// An object type that remembers its member order. `$key => field: Type`:
/// a field of type `Option<T>` is a member that may be absent.
#[macro_export]
macro_rules! js_object {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident {
            $(
                $(#[$fmeta:meta])*
                $key:literal => $fvis:vis $field:ident : $ty:ty
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq)]
        $vis struct $name {
            $( $(#[$fmeta])* $fvis $field: $ty, )*
            /// The order the members print in.
            #[doc(hidden)]
            pub order: $crate::object::Order,
        }

        impl $name {
            /// The members, in declaration order.
            pub const KEYS: &'static [&'static str] = &[$($key),*];

            /// Which members are present now, by declared key.
            pub fn present(&self) -> Vec<bool> {
                vec![$( $crate::object::Json::to_json(&self.$field).is_some() ),*]
            }

            /// Fix the member order to what is present now, so a member set
            /// later prints last, as a spread adds it.
            pub fn seal(&mut self) {
                let present = self.present();
                self.order.seal(&present);
            }

            /// Drop `key` from the member order (a rest destructure).
            pub fn forget_key(&mut self, key: &str) {
                self.seal();
                if let Some(i) = Self::KEYS.iter().position(|k| *k == key) {
                    self.order.forget(i);
                }
            }

            /// Set the order to `names` (a literal in its own order).
            pub fn ordered(mut self, names: &[&str]) -> Self {
                self.order = $crate::object::Order::named(names, Self::KEYS);
                self
            }

            /// Read the members of `object` (the tag of a tagged union excluded).
            pub fn from_object(object: &serde_json::Map<String, serde_json::Value>) -> Result<Self, $crate::object::JsonError> {
                Ok($name {
                    $( $field: $crate::object::Json::from_json(object.get($key)).map_err(|e| e.at($key))?, )*
                    order: $crate::object::Order::of(object, Self::KEYS),
                })
            }

            /// Print the members.
            pub fn to_object(&self) -> serde_json::Map<String, serde_json::Value> {
                self.order.print(Self::KEYS, vec![$( $crate::object::Json::to_json(&self.$field) ),*])
            }
        }

        impl $crate::object::Json for $name {
            fn from_json(value: Option<&serde_json::Value>) -> Result<Self, $crate::object::JsonError> {
                let object = value
                    .and_then(serde_json::Value::as_object)
                    .ok_or_else(|| $crate::object::JsonError::new(concat!("not a ", stringify!($name))))?;
                Self::from_object(object)
            }
            fn to_json(&self) -> Option<serde_json::Value> {
                Some(serde_json::Value::Object(self.to_object()))
            }
        }

        impl serde::Serialize for $name {
            /// The JSON the TypeScript object prints (`Json::to_json`).
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serde::Serialize::serialize(&$crate::object::Json::json(self), serializer)
            }
        }
    };
}

/// A union told apart by a tag member (`kind`, `type`), the tag printed
/// first: `"tag value" => Variant(Struct)`.
#[macro_export]
macro_rules! js_union {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident ($tag:literal) {
            $( $value:literal => $variant:ident ( $inner:ty ) ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq)]
        $vis enum $name {
            $( $variant($inner), )*
        }

        impl $name {
            /// The tag.
            pub fn tag(&self) -> &'static str {
                match self {
                    $( $name::$variant(_) => $value, )*
                }
            }
        }

        impl $crate::object::Json for $name {
            fn from_json(value: Option<&serde_json::Value>) -> Result<Self, $crate::object::JsonError> {
                let object = value
                    .and_then(serde_json::Value::as_object)
                    .ok_or_else(|| $crate::object::JsonError::new(concat!("not a ", stringify!($name))))?;
                match object.get($tag).and_then(serde_json::Value::as_str) {
                    $( Some($value) => <$inner>::from_object(object).map($name::$variant), )*
                    other => Err($crate::object::JsonError::new(format!(
                        concat!("unknown ", stringify!($name), " {:?}"),
                        other
                    ))),
                }
            }
            fn to_json(&self) -> Option<serde_json::Value> {
                let (tag, members) = match self {
                    $( $name::$variant(inner) => ($value, inner.to_object()), )*
                };
                let mut out = serde_json::Map::new();
                out.insert($tag.into(), serde_json::Value::String(tag.into()));
                for (k, v) in members {
                    out.insert(k, v);
                }
                Some(serde_json::Value::Object(out))
            }
        }

        impl serde::Serialize for $name {
            /// The JSON the TypeScript object prints (`Json::to_json`).
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serde::Serialize::serialize(&$crate::object::Json::json(self), serializer)
            }
        }
    };
}

/// A string union (`'baked' | 'vector'`).
#[macro_export]
macro_rules! js_strings {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $( $value:literal => $variant:ident ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        $vis enum $name {
            $( $variant, )*
        }

        impl $name {
            pub fn as_str(&self) -> &'static str {
                match self {
                    $( $name::$variant => $value, )*
                }
            }

            pub fn parse(text: &str) -> Option<Self> {
                match text {
                    $( $value => Some($name::$variant), )*
                    _ => None,
                }
            }
        }

        impl $crate::object::Json for $name {
            fn from_json(value: Option<&serde_json::Value>) -> Result<Self, $crate::object::JsonError> {
                let text = <String as $crate::object::Json>::from_json(value)?;
                Self::parse(&text)
                    .ok_or_else(|| $crate::object::JsonError::new(format!(concat!("not a ", stringify!($name), ": {}"), text)))
            }
            fn to_json(&self) -> Option<serde_json::Value> {
                Some(serde_json::Value::String(self.as_str().into()))
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_str())
            }
        }
    };
}

/// `JSON.stringify(a) === JSON.stringify(b)` on two printable values.
pub fn same_json<T: Json>(a: &T, b: &T) -> bool {
    json::stringify(&a.json()) == json::stringify(&b.json())
}

/// Read a value of `T` out of an object member.
pub fn member<T: Json>(object: &Object, key: &str) -> Result<T, JsonError> {
    T::from_json(object.get(key)).map_err(|e| e.at(key))
}
