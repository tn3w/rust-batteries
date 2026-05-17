use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::hash::Hash;

#[derive(Debug, Clone)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { f.write_str(&self.0) }
}

impl std::error::Error for Error {}

pub fn err(s: impl Into<String>) -> Error { Error(s.into()) }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind { Null, Bool, Num, Str, Seq, Map }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumKind { I64, U64, F64 }

pub trait Serializer {
    fn emit_null(&mut self) -> Result<(), Error>;
    fn emit_bool(&mut self, v: bool) -> Result<(), Error>;
    fn emit_i64(&mut self, v: i64) -> Result<(), Error>;
    fn emit_u64(&mut self, v: u64) -> Result<(), Error>;
    fn emit_f64(&mut self, v: f64) -> Result<(), Error>;
    fn emit_str(&mut self, v: &str) -> Result<(), Error>;
    fn begin_seq(&mut self) -> Result<(), Error>;
    fn end_seq(&mut self) -> Result<(), Error>;
    fn begin_map(&mut self) -> Result<(), Error>;
    fn key(&mut self, k: &str) -> Result<(), Error>;
    fn end_map(&mut self) -> Result<(), Error>;
}

pub trait Deserializer {
    fn peek(&mut self) -> Result<Kind, Error>;
    fn read_null(&mut self) -> Result<(), Error>;
    fn read_bool(&mut self) -> Result<bool, Error>;
    fn read_i64(&mut self) -> Result<i64, Error>;
    fn read_u64(&mut self) -> Result<u64, Error>;
    fn read_f64(&mut self) -> Result<f64, Error>;
    fn read_str(&mut self) -> Result<String, Error>;
    fn num_kind(&mut self) -> Result<NumKind, Error> { Ok(NumKind::F64) }
    fn begin_seq(&mut self) -> Result<(), Error>;
    fn seq_next(&mut self) -> Result<bool, Error>;
    fn begin_map(&mut self) -> Result<(), Error>;
    fn map_next(&mut self) -> Result<Option<String>, Error>;
    fn skip(&mut self) -> Result<(), Error>;
}

pub trait Serialize {
    fn serialize<S: Serializer>(&self, s: &mut S) -> Result<(), Error>;
}

pub trait Deserialize: Sized {
    fn deserialize<D: Deserializer>(d: &mut D) -> Result<Self, Error>;
}

impl Serialize for bool {
    fn serialize<S: Serializer>(&self, s: &mut S) -> Result<(), Error> { s.emit_bool(*self) }
}

impl Deserialize for bool {
    fn deserialize<D: Deserializer>(d: &mut D) -> Result<Self, Error> { d.read_bool() }
}

macro_rules! impl_int {
    ($($T:ty, $emit:ident, $read:ident, $As:ty);* $(;)?) => {
        $(
            impl Serialize for $T {
                fn serialize<S: Serializer>(&self, s: &mut S) -> Result<(), Error> {
                    s.$emit(*self as $As)
                }
            }
            impl Deserialize for $T {
                fn deserialize<D: Deserializer>(d: &mut D) -> Result<Self, Error> {
                    let v = d.$read()?;
                    <$T>::try_from(v).map_err(|_| err(concat!(stringify!($T), " out of range")))
                }
            }
        )*
    };
}

impl_int!(
    i8, emit_i64, read_i64, i64;
    i16, emit_i64, read_i64, i64;
    i32, emit_i64, read_i64, i64;
    i64, emit_i64, read_i64, i64;
    isize, emit_i64, read_i64, i64;
    u8, emit_u64, read_u64, u64;
    u16, emit_u64, read_u64, u64;
    u32, emit_u64, read_u64, u64;
    u64, emit_u64, read_u64, u64;
    usize, emit_u64, read_u64, u64;
);

impl Serialize for f32 {
    fn serialize<S: Serializer>(&self, s: &mut S) -> Result<(), Error> { s.emit_f64(*self as f64) }
}

impl Deserialize for f32 {
    fn deserialize<D: Deserializer>(d: &mut D) -> Result<Self, Error> { Ok(d.read_f64()? as f32) }
}

impl Serialize for f64 {
    fn serialize<S: Serializer>(&self, s: &mut S) -> Result<(), Error> { s.emit_f64(*self) }
}

impl Deserialize for f64 {
    fn deserialize<D: Deserializer>(d: &mut D) -> Result<Self, Error> { d.read_f64() }
}

impl Serialize for str {
    fn serialize<S: Serializer>(&self, s: &mut S) -> Result<(), Error> { s.emit_str(self) }
}

impl Serialize for String {
    fn serialize<S: Serializer>(&self, s: &mut S) -> Result<(), Error> { s.emit_str(self) }
}

impl Deserialize for String {
    fn deserialize<D: Deserializer>(d: &mut D) -> Result<Self, Error> { d.read_str() }
}

impl<T: Serialize + ?Sized> Serialize for &T {
    fn serialize<S: Serializer>(&self, s: &mut S) -> Result<(), Error> { (*self).serialize(s) }
}

impl<T: Serialize + ?Sized> Serialize for Box<T> {
    fn serialize<S: Serializer>(&self, s: &mut S) -> Result<(), Error> { (**self).serialize(s) }
}

impl<T: Deserialize> Deserialize for Box<T> {
    fn deserialize<D: Deserializer>(d: &mut D) -> Result<Self, Error> { Ok(Box::new(T::deserialize(d)?)) }
}

impl<T: Serialize> Serialize for Option<T> {
    fn serialize<S: Serializer>(&self, s: &mut S) -> Result<(), Error> {
        match self {
            None => s.emit_null(),
            Some(v) => v.serialize(s),
        }
    }
}

impl<T: Deserialize> Deserialize for Option<T> {
    fn deserialize<D: Deserializer>(d: &mut D) -> Result<Self, Error> {
        if d.peek()? == Kind::Null {
            d.read_null()?;
            return Ok(None);
        }
        Ok(Some(T::deserialize(d)?))
    }
}

impl<T: Serialize> Serialize for Vec<T> {
    fn serialize<S: Serializer>(&self, s: &mut S) -> Result<(), Error> {
        s.begin_seq()?;
        for x in self { x.serialize(s)?; }
        s.end_seq()
    }
}

impl<T: Serialize> Serialize for [T] {
    fn serialize<S: Serializer>(&self, s: &mut S) -> Result<(), Error> {
        s.begin_seq()?;
        for x in self { x.serialize(s)?; }
        s.end_seq()
    }
}

impl<T: Deserialize> Deserialize for Vec<T> {
    fn deserialize<D: Deserializer>(d: &mut D) -> Result<Self, Error> {
        d.begin_seq()?;
        let mut out = Vec::new();
        while d.seq_next()? { out.push(T::deserialize(d)?); }
        Ok(out)
    }
}

impl<V: Serialize> Serialize for BTreeMap<String, V> {
    fn serialize<S: Serializer>(&self, s: &mut S) -> Result<(), Error> {
        s.begin_map()?;
        for (k, v) in self { s.key(k)?; v.serialize(s)?; }
        s.end_map()
    }
}

impl<V: Deserialize> Deserialize for BTreeMap<String, V> {
    fn deserialize<D: Deserializer>(d: &mut D) -> Result<Self, Error> {
        d.begin_map()?;
        let mut out = BTreeMap::new();
        while let Some(k) = d.map_next()? { out.insert(k, V::deserialize(d)?); }
        Ok(out)
    }
}

impl<V: Serialize> Serialize for HashMap<String, V> {
    fn serialize<S: Serializer>(&self, s: &mut S) -> Result<(), Error> {
        s.begin_map()?;
        for (k, v) in self { s.key(k)?; v.serialize(s)?; }
        s.end_map()
    }
}

impl<V: Deserialize> Deserialize for HashMap<String, V>
where String: Eq + Hash
{
    fn deserialize<D: Deserializer>(d: &mut D) -> Result<Self, Error> {
        d.begin_map()?;
        let mut out = HashMap::new();
        while let Some(k) = d.map_next()? { out.insert(k, V::deserialize(d)?); }
        Ok(out)
    }
}

impl Serialize for () {
    fn serialize<S: Serializer>(&self, s: &mut S) -> Result<(), Error> { s.emit_null() }
}

impl Deserialize for () {
    fn deserialize<D: Deserializer>(d: &mut D) -> Result<Self, Error> { d.read_null() }
}

macro_rules! impl_tuple {
    ($($n:tt $T:ident),+) => {
        impl<$($T: Serialize),+> Serialize for ($($T,)+) {
            fn serialize<__S: Serializer>(&self, s: &mut __S) -> Result<(), Error> {
                s.begin_seq()?;
                $(self.$n.serialize(s)?;)+
                s.end_seq()
            }
        }
        impl<$($T: Deserialize),+> Deserialize for ($($T,)+) {
            fn deserialize<__D: Deserializer>(d: &mut __D) -> Result<Self, Error> {
                d.begin_seq()?;
                let out = (
                    $({
                        if !d.seq_next()? { return Err(err("tuple too short")); }
                        $T::deserialize(d)?
                    },)+
                );
                if d.seq_next()? { return Err(err("tuple too long")); }
                Ok(out)
            }
        }
    };
}

impl_tuple!(0 T0);
impl_tuple!(0 T0, 1 T1);
impl_tuple!(0 T0, 1 T1, 2 T2);
impl_tuple!(0 T0, 1 T1, 2 T2, 3 T3);
impl_tuple!(0 T0, 1 T1, 2 T2, 3 T3, 4 T4);
impl_tuple!(0 T0, 1 T1, 2 T2, 3 T3, 4 T4, 5 T5);

#[macro_export]
macro_rules! serde_struct {
    ($(#[$m:meta])* $vis:vis struct $Name:ident { $($fv:vis $f:ident : $T:ty),* $(,)? }) => {
        $(#[$m])*
        $vis struct $Name { $($fv $f: $T),* }
        $crate::serde_impl!($Name { $($f: $T),* });
    };
}

#[macro_export]
macro_rules! serde_impl {
    ($Name:ident { $($f:ident : $T:ty),* $(,)? }) => {
        impl $crate::serde::Serialize for $Name {
            fn serialize<S: $crate::serde::Serializer>(&self, s: &mut S)
                -> Result<(), $crate::serde::Error>
            {
                s.begin_map()?;
                $(
                    s.key(stringify!($f))?;
                    $crate::serde::Serialize::serialize(&self.$f, s)?;
                )*
                s.end_map()
            }
        }
        impl $crate::serde::Deserialize for $Name {
            fn deserialize<D: $crate::serde::Deserializer>(d: &mut D)
                -> Result<Self, $crate::serde::Error>
            {
                $(let mut $f: Option<$T> = None;)*
                d.begin_map()?;
                while let Some(__k) = d.map_next()? {
                    match __k.as_str() {
                        $(stringify!($f) => {
                            $f = Some(<$T as $crate::serde::Deserialize>::deserialize(d)?);
                        })*
                        _ => d.skip()?,
                    }
                }
                Ok(Self {
                    $($f: $f.ok_or_else(|| $crate::serde::err(
                        concat!("missing field `", stringify!($f), "`")
                    ))?),*
                })
            }
        }
    };
}
