//! Reading internally tagged enums (`{"do": "card", "card": {…}}`) without serde's buffering.
//!
//! serde's derived `#[serde(tag = "…")]` deserializer reads the whole object into a generic
//! `Content` tree before it knows the variant, then reads that tree again. A procedure card's
//! prompt embeddings and an intent's centroid are thousands of integers, so loading the index (on
//! every hook) spent most of its time there. [`deserialize`] reads the first key instead: when it
//! is the tag (always, for what Adage writes: serde writes the tag first), the rest of the object
//! is read straight into the variant. Otherwise the object is buffered as before, so any valid
//! input still reads the same. [`tagged_enum!`] declares such an enum once: its serialization is
//! the derived internally tagged one, unchanged.

use serde::de::{self, DeserializeSeed, EnumAccess, IgnoredAny, IntoDeserializer, MapAccess, VariantAccess, Visitor};
use serde::{Deserialize, Deserializer};
use std::fmt;
use std::marker::PhantomData;

/// Read an internally tagged enum whose tag is `tag`, as `R`: the same variants, externally
/// tagged (`{"<variant>": {…fields}}`), which [`tagged_enum!`] declares beside it.
pub fn deserialize<'de, D: Deserializer<'de>, R: Deserialize<'de>>(d: D, tag: &'static str) -> Result<R, D::Error> {
    d.deserialize_map(TagFirst { tag, out: PhantomData })
}

struct TagFirst<R> {
    tag: &'static str,
    out: PhantomData<R>,
}

impl<'de, R: Deserialize<'de>> Visitor<'de> for TagFirst<R> {
    type Value = R;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "a map with a `{}` field", self.tag)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<R, A::Error> {
        let first: Option<String> = map.next_key()?;
        if first.as_deref() == Some(self.tag) {
            let variant: String = map.next_value()?;
            return R::deserialize(Variant { variant, map });
        }
        // The tag is not first: buffer the object, as serde's derive does.
        let mut fields = serde_json::Map::new();
        if let Some(k) = first {
            fields.insert(k, map.next_value()?);
        }
        while let Some((k, v)) = map.next_entry::<String, serde_json::Value>()? {
            fields.insert(k, v);
        }
        let variant = match fields.remove(self.tag) {
            Some(serde_json::Value::String(s)) => s,
            Some(_) => return Err(de::Error::invalid_type(de::Unexpected::Map, &"a string tag")),
            None => return Err(de::Error::missing_field(self.tag)),
        };
        let external = serde_json::Value::Object([(variant, serde_json::Value::Object(fields))].into_iter().collect());
        R::deserialize(external).map_err(de::Error::custom)
    }
}

/// The rest of a tagged object, as the externally tagged variant `variant`.
struct Variant<A> {
    variant: String,
    map: A,
}

impl<'de, A: MapAccess<'de>> Deserializer<'de> for Variant<A> {
    type Error = A::Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, A::Error> {
        visitor.visit_enum(self)
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf option
        unit unit_struct newtype_struct seq tuple tuple_struct map struct enum identifier ignored_any
    }
}

impl<'de, A: MapAccess<'de>> EnumAccess<'de> for Variant<A> {
    type Error = A::Error;
    type Variant = Fields<A>;

    fn variant_seed<V: DeserializeSeed<'de>>(self, seed: V) -> Result<(V::Value, Fields<A>), A::Error> {
        let v = seed.deserialize(self.variant.into_deserializer())?;
        Ok((v, Fields(self.map)))
    }
}

/// A variant's fields: the object's remaining entries.
struct Fields<A>(A);

impl<'de, A: MapAccess<'de>> VariantAccess<'de> for Fields<A> {
    type Error = A::Error;

    fn unit_variant(mut self) -> Result<(), A::Error> {
        while self.0.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        Ok(())
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(self, _: T) -> Result<T::Value, A::Error> {
        Err(de::Error::invalid_type(de::Unexpected::NewtypeVariant, &"a struct variant"))
    }

    fn tuple_variant<V: Visitor<'de>>(self, _: usize, _: V) -> Result<V::Value, A::Error> {
        Err(de::Error::invalid_type(de::Unexpected::TupleVariant, &"a struct variant"))
    }

    fn struct_variant<V: Visitor<'de>>(self, _: &'static [&'static str], visitor: V) -> Result<V::Value, A::Error> {
        visitor.visit_map(self.0)
    }
}

/// Declare an internally tagged enum of unit and struct variants, serialized as serde derives it
/// (`#[serde(tag = $tag, rename_all = "snake_case")]`) and read by [`deserialize`] through a
/// private externally tagged mirror, `$repr`, with the same variants and field attributes.
macro_rules! tagged_enum {
    (
        $(#[$meta:meta])*
        pub enum $name:ident (tag = $tag:literal, mirror = $repr:ident) {
            $(
                $(#[$vmeta:meta])*
                $variant:ident $({ $( $(#[$fmeta:meta])* $field:ident : $fty:ty ),* $(,)? })?
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(serde::Serialize)]
        #[serde(tag = $tag, rename_all = "snake_case")]
        pub enum $name {
            $( $(#[$vmeta])* $variant $({ $( $(#[$fmeta])* $field: $fty ),* })? ),*
        }

        #[derive(serde::Deserialize)]
        #[serde(rename_all = "snake_case")]
        enum $repr {
            $( $variant $({ $( $(#[$fmeta])* $field: $fty ),* })? ),*
        }

        impl From<$repr> for $name {
            fn from(r: $repr) -> $name {
                match r {
                    $( $repr::$variant $({ $($field),* })? => $name::$variant $({ $($field),* })? ),*
                }
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<$name, D::Error> {
                $crate::tagged::deserialize::<D, $repr>(d, $tag).map(Into::into)
            }
        }
    };
}
pub(crate) use tagged_enum;

#[cfg(test)]
mod tests {
    use crate::model::{Step, Trigger};
    use serde_json::json;

    fn read<T: serde::de::DeserializeOwned>(v: serde_json::Value) -> Result<T, serde_json::Error> {
        // Through text, as the index is read (a `Value` would take a different path).
        serde_json::from_str(&v.to_string())
    }

    #[test]
    fn the_tag_may_come_first_or_later_and_reads_the_same() {
        let first: Step =
            read(json!({"do": "avoid", "avoid": "npm test", "instead": "make test", "signature": "x"})).unwrap();
        let later: Step =
            read(json!({"avoid": "npm test", "instead": "make test", "signature": "x", "do": "avoid"})).unwrap();
        assert_eq!(first, later);
        assert_eq!(first, Step::Avoid { avoid: "npm test".into(), instead: "make test".into(), signature: "x".into() });
        let i = json!({"family": "f", "model": "m", "centroid": [1, -2, 3], "threshold": 0.8});
        let mut first = json!({"on": "intent"});
        first.as_object_mut().unwrap().extend(i.as_object().unwrap().clone());
        let mut later = i.clone();
        later.as_object_mut().unwrap().insert("on".into(), json!("intent"));
        assert_eq!(read::<Trigger>(first).unwrap(), read::<Trigger>(later).unwrap());
    }

    #[test]
    fn unit_variants_ignore_other_keys_and_defaults_apply() {
        assert_eq!(read::<Trigger>(json!({"on": "session_start"})).unwrap(), Trigger::SessionStart);
        assert_eq!(read::<Trigger>(json!({"on": "session_start", "x": 1})).unwrap(), Trigger::SessionStart);
        assert_eq!(read::<Trigger>(json!({"x": 1, "on": "session_start"})).unwrap(), Trigger::SessionStart);
        let advise: Step = read(json!({"do": "advise", "template": "lockfile"})).unwrap();
        assert_eq!(advise, Step::Advise { template: "lockfile".into(), slots: Default::default() });
    }

    #[test]
    fn serialization_is_unchanged_and_round_trips() {
        let s = Step::Sequence { commands: vec!["make gen".into(), "make test".into()] };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v, json!({"do": "sequence", "commands": ["make gen", "make test"]}));
        assert_eq!(read::<Step>(v).unwrap(), s);
        let t = Trigger::AfterEdit { paths: "schema/*.json".into() };
        assert_eq!(serde_json::to_string(&t).unwrap(), r#"{"on":"after_edit","paths":"schema/*.json"}"#);
        // TOML, as tracks are written.
        #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
        struct W {
            step: Step,
        }
        let w = W { step: s };
        assert_eq!(toml::from_str::<W>(&toml::to_string(&w).unwrap()).unwrap(), w);
    }

    #[test]
    fn an_unknown_variant_or_a_missing_tag_is_an_error() {
        assert!(read::<Step>(json!({"do": "explode", "command": "x"})).is_err());
        assert!(read::<Step>(json!({"command": "x"})).is_err());
        assert!(read::<Step>(json!({"do": "run"})).is_err(), "a missing field");
        assert!(read::<Step>(json!({"do": 3, "command": "x"})).is_err());
    }
}
