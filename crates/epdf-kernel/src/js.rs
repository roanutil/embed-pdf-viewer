//! Serde across canonical JSON: the kernel prints actions and state with
//! it, and the conformance harness reads vector arguments with it.
//!
//! `serde_json` cannot carry `NaN` or the infinities: it serializes them as
//! `null` and refuses to read them. Canonical JSON spells them
//! `{"$number": "NaN" | "Infinity" | "-Infinity"}` and spells `undefined`
//! `{"$undefined": true}` (`canonical.ts`). [`to_value`] and [`from_value`]
//! wrap `serde_json`'s value serializer and deserializer with exactly those
//! two rules, so a port's plain `#[derive(Serialize, Deserialize)]` types
//! with `f64` members cross the harness without any per-field attribute.

use std::fmt;

use serde::de::{
    self, DeserializeOwned, DeserializeSeed, EnumAccess, IntoDeserializer, MapAccess, SeqAccess,
    VariantAccess, Visitor,
};
use serde::ser::{self, Serialize, SerializeMap, Serializer};
use serde_json::{Map, Value};

// ── Serialize ───────────────────────────────────────────────────────────────

/// Serialize to canonical JSON: a non-finite float becomes `{"$number": ..}`.
pub fn to_value<T: Serialize + ?Sized>(value: &T) -> Value {
    value
        .serialize(JsSerializer(serde_json::value::Serializer))
        .unwrap_or_else(|e| panic!("[js] serialize: {e}"))
}

/// The canonical spelling of a number.
pub fn number(value: f64) -> Value {
    if value.is_nan() {
        serde_json::json!({ "$number": "NaN" })
    } else if value == f64::INFINITY {
        serde_json::json!({ "$number": "Infinity" })
    } else if value == f64::NEG_INFINITY {
        serde_json::json!({ "$number": "-Infinity" })
    } else {
        serde_json::json!(value)
    }
}

/// A number as `JSON.stringify` writes it, in canonical JSON: a whole
/// number in the safe-integer range without a fraction (`1`, not `1.0`),
/// `-0` as `0`, and the non-finite spelled as [`number`] spells them.
pub fn json_number(value: f64) -> Value {
    const SAFE: f64 = 9_007_199_254_740_991.0;
    if value.fract() == 0.0 && value.abs() <= SAFE {
        Value::from(value as i64)
    } else {
        number(value)
    }
}

/// `Number.prototype.toString()` (ECMAScript `Number::toString`, radix 10):
/// the shortest round-trip digits, laid out in plain notation for exponents
/// from -7 to 20 and scientific (`1e+21`, `1.5e-7`) outside it. Rust's `{}`
/// never switches to scientific notation and prints `-0` as `-0`.
/// `epdf_geometry::js` carries the same function; that crate depends on this
/// one, so the kernel keeps its own copy.
pub fn number_to_string(x: f64) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x == 0.0 {
        return "0".into();
    }
    if x < 0.0 {
        return format!("-{}", number_to_string(-x));
    }
    if x.is_infinite() {
        return "Infinity".into();
    }
    // `{:e}` gives the shortest round-trip digits as `d[.ddd]e<exp>`.
    let sci = format!("{x:e}");
    let (mantissa, exponent) = sci.split_once('e').expect("LowerExp has an exponent");
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    let n = exponent.parse::<i32>().expect("integer exponent") + 1;
    if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        let (int, frac) = digits.split_at(n as usize);
        format!("{int}.{frac}")
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let e = n - 1;
        let sign = if e < 0 { '-' } else { '+' };
        let (first, rest) = digits.split_at(1);
        if rest.is_empty() {
            format!("{first}e{sign}{}", e.abs())
        } else {
            format!("{first}.{rest}e{sign}{}", e.abs())
        }
    }
}

struct Js<'a, T: ?Sized>(&'a T);

impl<T: Serialize + ?Sized> Serialize for Js<'_, T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(JsSerializer(serializer))
    }
}

struct JsSerializer<S>(S);
struct Compound<C>(C);

fn non_finite<S: Serializer>(serializer: S, value: f64) -> Result<S::Ok, S::Error> {
    let text = if value.is_nan() {
        "NaN"
    } else if value > 0.0 {
        "Infinity"
    } else {
        "-Infinity"
    };
    let mut map = serializer.serialize_map(Some(1))?;
    map.serialize_entry("$number", text)?;
    map.end()
}

impl<S: Serializer> Serializer for JsSerializer<S> {
    type Ok = S::Ok;
    type Error = S::Error;
    type SerializeSeq = Compound<S::SerializeSeq>;
    type SerializeTuple = Compound<S::SerializeTuple>;
    type SerializeTupleStruct = Compound<S::SerializeTupleStruct>;
    type SerializeTupleVariant = Compound<S::SerializeTupleVariant>;
    type SerializeMap = Compound<S::SerializeMap>;
    type SerializeStruct = Compound<S::SerializeStruct>;
    type SerializeStructVariant = Compound<S::SerializeStructVariant>;

    fn serialize_bool(self, v: bool) -> Result<S::Ok, S::Error> {
        self.0.serialize_bool(v)
    }
    fn serialize_i8(self, v: i8) -> Result<S::Ok, S::Error> {
        self.0.serialize_i8(v)
    }
    fn serialize_i16(self, v: i16) -> Result<S::Ok, S::Error> {
        self.0.serialize_i16(v)
    }
    fn serialize_i32(self, v: i32) -> Result<S::Ok, S::Error> {
        self.0.serialize_i32(v)
    }
    fn serialize_i64(self, v: i64) -> Result<S::Ok, S::Error> {
        self.0.serialize_i64(v)
    }
    fn serialize_u8(self, v: u8) -> Result<S::Ok, S::Error> {
        self.0.serialize_u8(v)
    }
    fn serialize_u16(self, v: u16) -> Result<S::Ok, S::Error> {
        self.0.serialize_u16(v)
    }
    fn serialize_u32(self, v: u32) -> Result<S::Ok, S::Error> {
        self.0.serialize_u32(v)
    }
    fn serialize_u64(self, v: u64) -> Result<S::Ok, S::Error> {
        self.0.serialize_u64(v)
    }
    fn serialize_f32(self, v: f32) -> Result<S::Ok, S::Error> {
        self.serialize_f64(f64::from(v))
    }
    fn serialize_f64(self, v: f64) -> Result<S::Ok, S::Error> {
        if v.is_finite() { self.0.serialize_f64(v) } else { non_finite(self.0, v) }
    }
    fn serialize_char(self, v: char) -> Result<S::Ok, S::Error> {
        self.0.serialize_char(v)
    }
    fn serialize_str(self, v: &str) -> Result<S::Ok, S::Error> {
        self.0.serialize_str(v)
    }
    fn serialize_bytes(self, v: &[u8]) -> Result<S::Ok, S::Error> {
        self.0.serialize_bytes(v)
    }
    fn serialize_none(self) -> Result<S::Ok, S::Error> {
        self.0.serialize_none()
    }
    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<S::Ok, S::Error> {
        self.0.serialize_some(&Js(value))
    }
    fn serialize_unit(self) -> Result<S::Ok, S::Error> {
        self.0.serialize_unit()
    }
    fn serialize_unit_struct(self, name: &'static str) -> Result<S::Ok, S::Error> {
        self.0.serialize_unit_struct(name)
    }
    fn serialize_unit_variant(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
    ) -> Result<S::Ok, S::Error> {
        self.0.serialize_unit_variant(name, index, variant)
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        name: &'static str,
        value: &T,
    ) -> Result<S::Ok, S::Error> {
        self.0.serialize_newtype_struct(name, &Js(value))
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<S::Ok, S::Error> {
        self.0.serialize_newtype_variant(name, index, variant, &Js(value))
    }
    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq, S::Error> {
        self.0.serialize_seq(len).map(Compound)
    }
    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple, S::Error> {
        self.0.serialize_tuple(len).map(Compound)
    }
    fn serialize_tuple_struct(
        self,
        name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleStruct, S::Error> {
        self.0.serialize_tuple_struct(name, len).map(Compound)
    }
    fn serialize_tuple_variant(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleVariant, S::Error> {
        self.0.serialize_tuple_variant(name, index, variant, len).map(Compound)
    }
    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap, S::Error> {
        self.0.serialize_map(len).map(Compound)
    }
    fn serialize_struct(
        self,
        name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStruct, S::Error> {
        self.0.serialize_struct(name, len).map(Compound)
    }
    fn serialize_struct_variant(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStructVariant, S::Error> {
        self.0.serialize_struct_variant(name, index, variant, len).map(Compound)
    }
}

impl<C: ser::SerializeSeq> ser::SerializeSeq for Compound<C> {
    type Ok = C::Ok;
    type Error = C::Error;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), C::Error> {
        self.0.serialize_element(&Js(value))
    }
    fn end(self) -> Result<C::Ok, C::Error> {
        self.0.end()
    }
}

impl<C: ser::SerializeTuple> ser::SerializeTuple for Compound<C> {
    type Ok = C::Ok;
    type Error = C::Error;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), C::Error> {
        self.0.serialize_element(&Js(value))
    }
    fn end(self) -> Result<C::Ok, C::Error> {
        self.0.end()
    }
}

impl<C: ser::SerializeTupleStruct> ser::SerializeTupleStruct for Compound<C> {
    type Ok = C::Ok;
    type Error = C::Error;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), C::Error> {
        self.0.serialize_field(&Js(value))
    }
    fn end(self) -> Result<C::Ok, C::Error> {
        self.0.end()
    }
}

impl<C: ser::SerializeTupleVariant> ser::SerializeTupleVariant for Compound<C> {
    type Ok = C::Ok;
    type Error = C::Error;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), C::Error> {
        self.0.serialize_field(&Js(value))
    }
    fn end(self) -> Result<C::Ok, C::Error> {
        self.0.end()
    }
}

impl<C: ser::SerializeMap> ser::SerializeMap for Compound<C> {
    type Ok = C::Ok;
    type Error = C::Error;
    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), C::Error> {
        self.0.serialize_key(key)
    }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), C::Error> {
        self.0.serialize_value(&Js(value))
    }
    fn end(self) -> Result<C::Ok, C::Error> {
        self.0.end()
    }
}

impl<C: ser::SerializeStruct> ser::SerializeStruct for Compound<C> {
    type Ok = C::Ok;
    type Error = C::Error;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), C::Error> {
        self.0.serialize_field(key, &Js(value))
    }
    fn skip_field(&mut self, key: &'static str) -> Result<(), C::Error> {
        self.0.skip_field(key)
    }
    fn end(self) -> Result<C::Ok, C::Error> {
        self.0.end()
    }
}

impl<C: ser::SerializeStructVariant> ser::SerializeStructVariant for Compound<C> {
    type Ok = C::Ok;
    type Error = C::Error;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), C::Error> {
        self.0.serialize_field(key, &Js(value))
    }
    fn skip_field(&mut self, key: &'static str) -> Result<(), C::Error> {
        self.0.skip_field(key)
    }
    fn end(self) -> Result<C::Ok, C::Error> {
        self.0.end()
    }
}

// ── Deserialize ─────────────────────────────────────────────────────────────

/// Deserialize from canonical JSON: `{"$number": ..}` reads as a float and
/// `{"$undefined": true}` as an absent value (`None` for an `Option`).
pub fn from_value<T: DeserializeOwned>(value: &Value) -> Result<T, crate::Error> {
    T::deserialize(JsDeserializer(value))
        .map_err(|e| crate::Error::new(format!("[js] cannot decode {value}: {e}")))
}

/// Argument `index` of a function case. A missing trailing argument is
/// `undefined`, as in a JavaScript call.
pub fn arg<T: DeserializeOwned>(args: &[Value], index: usize) -> Result<T, crate::Error> {
    let undefined = crate::undefined();
    from_value(args.get(index).unwrap_or(&undefined))
}

#[derive(Debug)]
pub struct DecodeError(String);

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DecodeError {}

impl de::Error for DecodeError {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        DecodeError(msg.to_string())
    }
}

fn special_number(map: &Map<String, Value>) -> Option<f64> {
    if map.len() != 1 {
        return None;
    }
    match map.get("$number")?.as_str()? {
        "NaN" => Some(f64::NAN),
        "Infinity" => Some(f64::INFINITY),
        "-Infinity" => Some(f64::NEG_INFINITY),
        _ => None,
    }
}

fn is_absent(value: &Value) -> bool {
    value.is_null() || crate::is_undefined(value)
}

#[derive(Clone, Copy)]
struct JsDeserializer<'a>(&'a Value);

impl<'de> de::Deserializer<'de> for JsDeserializer<'_> {
    type Error = DecodeError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DecodeError> {
        match self.0 {
            Value::Null => visitor.visit_unit(),
            Value::Bool(b) => visitor.visit_bool(*b),
            Value::Number(n) => {
                if let Some(u) = n.as_u64() {
                    visitor.visit_u64(u)
                } else if let Some(i) = n.as_i64() {
                    visitor.visit_i64(i)
                } else {
                    visitor.visit_f64(n.as_f64().unwrap_or(f64::NAN))
                }
            }
            Value::String(s) => visitor.visit_str(s),
            Value::Array(items) => visitor.visit_seq(Seq(items.iter())),
            Value::Object(map) => {
                if let Some(n) = special_number(map) {
                    return visitor.visit_f64(n);
                }
                if crate::is_undefined(self.0) {
                    return visitor.visit_unit();
                }
                visitor.visit_map(Entries { iter: map.iter(), value: None })
            }
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DecodeError> {
        if is_absent(self.0) { visitor.visit_none() } else { visitor.visit_some(self) }
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, DecodeError> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, DecodeError> {
        match self.0 {
            Value::String(s) => visitor.visit_enum(s.as_str().into_deserializer()),
            Value::Object(map) if map.len() == 1 => {
                let (variant, value) = map.iter().next().expect("one entry");
                visitor.visit_enum(Variant { variant, value })
            }
            other => Err(de::Error::custom(format!("expected an enum, got {other}"))),
        }
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf unit unit_struct seq tuple tuple_struct map struct
        identifier ignored_any
    }
}

struct Seq<'a>(std::slice::Iter<'a, Value>);

impl<'de> SeqAccess<'de> for Seq<'_> {
    type Error = DecodeError;
    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, DecodeError> {
        self.0.next().map(|v| seed.deserialize(JsDeserializer(v))).transpose()
    }
    fn size_hint(&self) -> Option<usize> {
        Some(self.0.len())
    }
}

struct Entries<'a> {
    iter: serde_json::map::Iter<'a>,
    value: Option<&'a Value>,
}

impl<'de> MapAccess<'de> for Entries<'_> {
    type Error = DecodeError;
    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, DecodeError> {
        match self.iter.next() {
            Some((key, value)) => {
                self.value = Some(value);
                seed.deserialize(key.as_str().into_deserializer()).map(Some)
            }
            None => Ok(None),
        }
    }
    fn next_value_seed<V: DeserializeSeed<'de>>(
        &mut self,
        seed: V,
    ) -> Result<V::Value, DecodeError> {
        let value = self.value.take().ok_or_else(|| de::Error::custom("value before key"))?;
        seed.deserialize(JsDeserializer(value))
    }
}

struct Variant<'a> {
    variant: &'a str,
    value: &'a Value,
}

impl<'de, 'a> EnumAccess<'de> for Variant<'a> {
    type Error = DecodeError;
    type Variant = JsDeserializer<'a>;
    fn variant_seed<V: DeserializeSeed<'de>>(
        self,
        seed: V,
    ) -> Result<(V::Value, Self::Variant), DecodeError> {
        let variant = seed.deserialize(self.variant.into_deserializer())?;
        Ok((variant, JsDeserializer(self.value)))
    }
}

impl<'de> VariantAccess<'de> for JsDeserializer<'_> {
    type Error = DecodeError;
    fn unit_variant(self) -> Result<(), DecodeError> {
        Ok(())
    }
    fn newtype_variant_seed<T: DeserializeSeed<'de>>(
        self,
        seed: T,
    ) -> Result<T::Value, DecodeError> {
        seed.deserialize(self)
    }
    fn tuple_variant<V: Visitor<'de>>(
        self,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, DecodeError> {
        de::Deserializer::deserialize_any(self, visitor)
    }
    fn struct_variant<V: Visitor<'de>>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, DecodeError> {
        de::Deserializer::deserialize_any(self, visitor)
    }
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};
    use serde_json::json;

    use super::*;

    #[test]
    fn json_number_writes_as_json_stringify_does() {
        assert_eq!(json_number(1.0).to_string(), "1");
        assert_eq!(json_number(-0.0).to_string(), "0");
        assert_eq!(json_number(1.5).to_string(), "1.5");
        assert_eq!(json_number(9_007_199_254_740_991.0).to_string(), "9007199254740991");
        assert_eq!(json_number(9_007_199_254_740_992.0).to_string(), "9007199254740992.0");
        assert_eq!(json_number(f64::NAN), json!({ "$number": "NaN" }));
        assert_eq!(json_number(f64::NEG_INFINITY), json!({ "$number": "-Infinity" }));
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct P {
        x: f64,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        y: Option<f64>,
    }

    #[test]
    fn non_finite_numbers_round_trip() {
        let p = P { x: f64::NEG_INFINITY, y: Some(f64::INFINITY) };
        let v = to_value(&p);
        assert_eq!(v, json!({"x": {"$number": "-Infinity"}, "y": {"$number": "Infinity"}}));
        let back: P = from_value(&v).unwrap();
        assert_eq!(back, p);
        let nan: P = from_value(&json!({"x": {"$number": "NaN"}})).unwrap();
        assert!(nan.x.is_nan() && nan.y.is_none());
        assert_eq!(to_value(&[f64::NAN]), json!([{"$number": "NaN"}]));
    }

    #[test]
    fn number_to_string_matches_javascript() {
        let cases: &[(f64, &str)] = &[
            (1.0, "1"),
            (-0.0, "0"),
            (0.1, "0.1"),
            (1e20, "100000000000000000000"),
            (1e21, "1e+21"),
            (1.5e21, "1.5e+21"),
            (0.000001, "0.000001"),
            (1e-7, "1e-7"),
            (1.2345e-7, "1.2345e-7"),
            (f64::INFINITY, "Infinity"),
            (f64::NEG_INFINITY, "-Infinity"),
            (f64::NAN, "NaN"),
            (-12.5, "-12.5"),
        ];
        for (x, want) in cases {
            assert_eq!(number_to_string(*x), *want, "{x}");
        }
    }

    #[test]
    fn number_spells_the_non_finite_values() {
        assert_eq!(number(f64::NAN), json!({"$number": "NaN"}));
        assert_eq!(number(f64::INFINITY), json!({"$number": "Infinity"}));
        assert_eq!(number(f64::NEG_INFINITY), json!({"$number": "-Infinity"}));
        assert_eq!(number(1.5), json!(1.5));
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct Pair(f64, f64);

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    enum Shape {
        Unit,
        Point(f64),
        Pair(f64, f64),
        Named { x: f64 },
    }

    #[test]
    fn every_compound_carries_its_members() {
        assert_eq!(to_value(&Pair(1.0, f64::NAN)), json!([1.0, {"$number": "NaN"}]));
        assert_eq!(to_value(&Shape::Pair(1.0, 2.0)), json!({"Pair": [1.0, 2.0]}));
        assert_eq!(
            to_value(&Shape::Named { x: f64::INFINITY }),
            json!({"Named": {"x": {"$number": "Infinity"}}})
        );
    }

    #[test]
    fn sequences_and_enums_read_back() {
        let list: Vec<f64> = from_value(&json!([1, {"$number": "NaN"}, 3])).unwrap();
        assert_eq!((list.len(), list[0], list[2]), (3, 1.0, 3.0));
        assert!(list[1].is_nan());
        assert_eq!(from_value::<Pair>(&json!([1, 2])).unwrap(), Pair(1.0, 2.0));
        assert_eq!(from_value::<Shape>(&json!("Unit")).unwrap(), Shape::Unit);
        assert_eq!(from_value::<Shape>(&json!({"Point": 4})).unwrap(), Shape::Point(4.0));
        assert_eq!(
            from_value::<Shape>(&json!({"Named": {"x": 2}})).unwrap(),
            Shape::Named { x: 2.0 }
        );
        // An enum is one key; two is not a variant, whichever comes first.
        let error = from_value::<Shape>(&json!({"Point": 4, "Unit": null})).unwrap_err();
        assert!(error.message.contains("expected an enum"), "{}", error.message);
    }

    #[test]
    fn undefined_reads_as_none() {
        let p: P = from_value(&json!({"x": 1, "y": {"$undefined": true}})).unwrap();
        assert_eq!(p, P { x: 1.0, y: None });
        let missing: Option<f64> = arg(&[], 0).unwrap();
        assert_eq!(missing, None);
    }
}
