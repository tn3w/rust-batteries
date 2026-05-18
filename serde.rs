use std::borrow::Cow;
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

pub trait Deserializer<'de> {
    fn peek(&mut self) -> Result<Kind, Error>;
    fn read_null(&mut self) -> Result<(), Error>;
    fn read_bool(&mut self) -> Result<bool, Error>;
    fn read_i64(&mut self) -> Result<i64, Error>;
    fn read_u64(&mut self) -> Result<u64, Error>;
    fn read_f64(&mut self) -> Result<f64, Error>;
    fn read_str(&mut self) -> Result<String, Error>;
    fn read_str_borrowed(&mut self) -> Result<&'de str, Error> {
        Err(err("borrowed str unsupported by this deserializer"))
    }
    fn read_str_cow(&mut self) -> Result<Cow<'de, str>, Error> {
        self.read_str().map(Cow::Owned)
    }
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

pub trait Deserialize<'de>: Sized {
    fn deserialize<D: Deserializer<'de>>(d: &mut D) -> Result<Self, Error>;
}

impl Serialize for bool {
    fn serialize<S: Serializer>(&self, s: &mut S) -> Result<(), Error> { s.emit_bool(*self) }
}

impl<'de> Deserialize<'de> for bool {
    fn deserialize<D: Deserializer<'de>>(d: &mut D) -> Result<Self, Error> { d.read_bool() }
}

macro_rules! impl_int {
    ($($T:ty, $emit:ident, $read:ident, $As:ty);* $(;)?) => {
        $(
            impl Serialize for $T {
                fn serialize<S: Serializer>(&self, s: &mut S) -> Result<(), Error> {
                    s.$emit(*self as $As)
                }
            }
            impl<'de> Deserialize<'de> for $T {
                fn deserialize<D: Deserializer<'de>>(d: &mut D) -> Result<Self, Error> {
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

impl<'de> Deserialize<'de> for f32 {
    fn deserialize<D: Deserializer<'de>>(d: &mut D) -> Result<Self, Error> { Ok(d.read_f64()? as f32) }
}

impl Serialize for f64 {
    fn serialize<S: Serializer>(&self, s: &mut S) -> Result<(), Error> { s.emit_f64(*self) }
}

impl<'de> Deserialize<'de> for f64 {
    fn deserialize<D: Deserializer<'de>>(d: &mut D) -> Result<Self, Error> { d.read_f64() }
}

impl Serialize for str {
    fn serialize<S: Serializer>(&self, s: &mut S) -> Result<(), Error> { s.emit_str(self) }
}

impl Serialize for String {
    fn serialize<S: Serializer>(&self, s: &mut S) -> Result<(), Error> { s.emit_str(self) }
}

impl<'de> Deserialize<'de> for String {
    fn deserialize<D: Deserializer<'de>>(d: &mut D) -> Result<Self, Error> { d.read_str() }
}

impl<'de> Deserialize<'de> for &'de str {
    fn deserialize<D: Deserializer<'de>>(d: &mut D) -> Result<Self, Error> { d.read_str_borrowed() }
}

impl<'de> Deserialize<'de> for Cow<'de, str> {
    fn deserialize<D: Deserializer<'de>>(d: &mut D) -> Result<Self, Error> { d.read_str_cow() }
}

impl<T: Serialize + ?Sized> Serialize for &T {
    fn serialize<S: Serializer>(&self, s: &mut S) -> Result<(), Error> { (*self).serialize(s) }
}

impl<T: Serialize + ?Sized> Serialize for Box<T> {
    fn serialize<S: Serializer>(&self, s: &mut S) -> Result<(), Error> { (**self).serialize(s) }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Box<T> {
    fn deserialize<D: Deserializer<'de>>(d: &mut D) -> Result<Self, Error> {
        Ok(Box::new(T::deserialize(d)?))
    }
}

impl<T: Serialize> Serialize for Option<T> {
    fn serialize<S: Serializer>(&self, s: &mut S) -> Result<(), Error> {
        match self {
            None => s.emit_null(),
            Some(v) => v.serialize(s),
        }
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Option<T> {
    fn deserialize<D: Deserializer<'de>>(d: &mut D) -> Result<Self, Error> {
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

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Vec<T> {
    fn deserialize<D: Deserializer<'de>>(d: &mut D) -> Result<Self, Error> {
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

impl<'de, V: Deserialize<'de>> Deserialize<'de> for BTreeMap<String, V> {
    fn deserialize<D: Deserializer<'de>>(d: &mut D) -> Result<Self, Error> {
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

impl<'de, V: Deserialize<'de>> Deserialize<'de> for HashMap<String, V>
where String: Eq + Hash
{
    fn deserialize<D: Deserializer<'de>>(d: &mut D) -> Result<Self, Error> {
        d.begin_map()?;
        let mut out = HashMap::new();
        while let Some(k) = d.map_next()? { out.insert(k, V::deserialize(d)?); }
        Ok(out)
    }
}

impl Serialize for () {
    fn serialize<S: Serializer>(&self, s: &mut S) -> Result<(), Error> { s.emit_null() }
}

impl<'de> Deserialize<'de> for () {
    fn deserialize<D: Deserializer<'de>>(d: &mut D) -> Result<Self, Error> { d.read_null() }
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
        impl<'de, $($T: Deserialize<'de>),+> Deserialize<'de> for ($($T,)+) {
            fn deserialize<__D: Deserializer<'de>>(d: &mut __D) -> Result<Self, Error> {
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
    ($(#[$m:meta])* $vis:vis struct $Name:ident<$lt:lifetime> {
        $($fv:vis $f:ident $(as $rn:literal)? : $T:ty),* $(,)?
    }) => {
        $(#[$m])*
        $vis struct $Name<$lt> { $($fv $f: $T),* }
        $crate::serde_impl_lt!($Name, $lt, { $($f $(as $rn)? : $T),* });
    };
    ($(#[$m:meta])* $vis:vis struct $Name:ident {
        $($fv:vis $f:ident $(as $rn:literal)? : $T:ty),* $(,)?
    }) => {
        $(#[$m])*
        $vis struct $Name { $($fv $f: $T),* }
        $crate::serde_impl!($Name, { $($f $(as $rn)? : $T),* });
    };
}

#[macro_export]
macro_rules! serde_impl {
    ($Name:ident, { $($f:ident $(as $rn:literal)? : $T:ty),* $(,)? }) => {
        impl $crate::serde::Serialize for $Name {
            fn serialize<__S: $crate::serde::Serializer>(&self, __s: &mut __S)
                -> Result<(), $crate::serde::Error>
            {
                __s.begin_map()?;
                $({
                    let __n = stringify!($f);
                    $(let __n = $rn;)?
                    __s.key(__n)?;
                    $crate::serde::Serialize::serialize(&self.$f, __s)?;
                })*
                __s.end_map()
            }
        }
        impl<'de> $crate::serde::Deserialize<'de> for $Name {
            fn deserialize<__D: $crate::serde::Deserializer<'de>>(__d: &mut __D)
                -> Result<Self, $crate::serde::Error>
            {
                $(let mut $f: Option<$T> = None;)*
                __d.begin_map()?;
                while let Some(__k) = __d.map_next()? {
                    let __k = __k.as_str();
                    let mut __matched = false;
                    $({
                        let __n = stringify!($f);
                        $(let __n = $rn;)?
                        if !__matched && __k == __n {
                            $f = Some(<$T as $crate::serde::Deserialize<'de>>::deserialize(__d)?);
                            __matched = true;
                        }
                    })*
                    if !__matched { __d.skip()?; }
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

#[macro_export]
macro_rules! serde_impl_lt {
    ($Name:ident, $lt:lifetime, { $($f:ident $(as $rn:literal)? : $T:ty),* $(,)? }) => {
        impl<$lt> $crate::serde::Serialize for $Name<$lt> {
            fn serialize<__S: $crate::serde::Serializer>(&self, __s: &mut __S)
                -> Result<(), $crate::serde::Error>
            {
                __s.begin_map()?;
                $({
                    let __n = stringify!($f);
                    $(let __n = $rn;)?
                    __s.key(__n)?;
                    $crate::serde::Serialize::serialize(&self.$f, __s)?;
                })*
                __s.end_map()
            }
        }
        impl<$lt> $crate::serde::Deserialize<$lt> for $Name<$lt> {
            fn deserialize<__D: $crate::serde::Deserializer<$lt>>(__d: &mut __D)
                -> Result<Self, $crate::serde::Error>
            {
                $(let mut $f: Option<$T> = None;)*
                __d.begin_map()?;
                while let Some(__k) = __d.map_next()? {
                    let __k = __k.as_str();
                    let mut __matched = false;
                    $({
                        let __n = stringify!($f);
                        $(let __n = $rn;)?
                        if !__matched && __k == __n {
                            $f = Some(<$T as $crate::serde::Deserialize<$lt>>::deserialize(__d)?);
                            __matched = true;
                        }
                    })*
                    if !__matched { __d.skip()?; }
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
#[macro_export]
macro_rules! serde_enum {
    ($(#[$m:meta])* $vis:vis enum $Name:ident { $($body:tt)* }) => {
        $crate::__se_enum_go!(
            @s __s @d __d @n __name
            ($(#[$m])*) $vis $Name
            [] [] [] []
            $($body)* ,
        );
    };
}

#[macro_export]
macro_rules! __se_enum_go {
    (@s $s:ident @d $d:ident @n $n:ident
     ($($m:tt)*) $vis:vis $Name:ident
     [$($eb:tt)*] [$($ser:tt)*] [$($udx:tt)*] [$($ddx:tt)*]
     $(,)?
    ) => {
        $($m)*
        $vis enum $Name { $($eb)* }

        impl $crate::serde::Serialize for $Name {
            fn serialize<__S: $crate::serde::Serializer>(&self, $s: &mut __S)
                -> Result<(), $crate::serde::Error>
            {
                match self { $($ser)* }
            }
        }

        impl<'de> $crate::serde::Deserialize<'de> for $Name {
            fn deserialize<__D: $crate::serde::Deserializer<'de>>($d: &mut __D)
                -> Result<Self, $crate::serde::Error>
            {
                match $d.peek()? {
                    $crate::serde::Kind::Str => {
                        let $n = $d.read_str()?;
                        let $n = $n.as_str();
                        $($udx)*
                        Err($crate::serde::err("unknown unit variant"))
                    }
                    $crate::serde::Kind::Map => {
                        $d.begin_map()?;
                        let $n = $d.map_next()?
                            .ok_or_else(|| $crate::serde::err("empty variant map"))?;
                        let $n = $n.as_str();
                        let __out: Result<Self, $crate::serde::Error> = (|| {
                            $($ddx)*
                            Err($crate::serde::err("unknown variant"))
                        })();
                        let __val = __out?;
                        if $d.map_next()?.is_some() {
                            return Err($crate::serde::err("extra fields after variant"));
                        }
                        Ok(__val)
                    }
                    _ => Err($crate::serde::err("expected enum variant")),
                }
            }
        }
    };

    (@s $s:ident @d $d:ident @n $n:ident
     ($($m:tt)*) $vis:vis $Name:ident
     [$($eb:tt)*] [$($ser:tt)*] [$($udx:tt)*] [$($ddx:tt)*]
     $V:ident $(as $rn:literal)? , $($rest:tt)*
    ) => {
        $crate::__se_enum_go!(
            @s $s @d $d @n $n
            ($($m)*) $vis $Name
            [$($eb)* $V,]
            [$($ser)* Self::$V => {
                let __n = stringify!($V); $(let __n = $rn;)?
                $s.emit_str(__n)
            },]
            [$($udx)* {
                let __n = stringify!($V); $(let __n = $rn;)?
                if $n == __n { return Ok(Self::$V); }
            }]
            [$($ddx)*]
            $($rest)*
        );
    };

    (@s $s:ident @d $d:ident @n $n:ident
     ($($m:tt)*) $vis:vis $Name:ident
     [$($eb:tt)*] [$($ser:tt)*] [$($udx:tt)*] [$($ddx:tt)*]
     $V:ident $(as $rn:literal)? ( $T:ty $(,)? ) , $($rest:tt)*
    ) => {
        $crate::__se_enum_go!(
            @s $s @d $d @n $n
            ($($m)*) $vis $Name
            [$($eb)* $V($T),]
            [$($ser)* Self::$V(__x) => {
                let __n = stringify!($V); $(let __n = $rn;)?
                $s.begin_map()?;
                $s.key(__n)?;
                <$T as $crate::serde::Serialize>::serialize(__x, $s)?;
                $s.end_map()
            },]
            [$($udx)*]
            [$($ddx)* {
                let __n = stringify!($V); $(let __n = $rn;)?
                if $n == __n {
                    return Ok(Self::$V(
                        <$T as $crate::serde::Deserialize<'de>>::deserialize($d)?
                    ));
                }
            }]
            $($rest)*
        );
    };

    (@s $s:ident @d $d:ident @n $n:ident
     ($($m:tt)*) $vis:vis $Name:ident
     [$($eb:tt)*] [$($ser:tt)*] [$($udx:tt)*] [$($ddx:tt)*]
     $V:ident $(as $rn:literal)? { $($f:ident $(as $frn:literal)? : $Tf:ty),+ $(,)? } , $($rest:tt)*
    ) => {
        $crate::__se_enum_go!(
            @s $s @d $d @n $n
            ($($m)*) $vis $Name
            [$($eb)* $V { $($f: $Tf),+ },]
            [$($ser)* Self::$V { $($f),+ } => {
                let __n = stringify!($V); $(let __n = $rn;)?
                $s.begin_map()?;
                $s.key(__n)?;
                $s.begin_map()?;
                $({
                    let __fn = stringify!($f); $(let __fn = $frn;)?
                    $s.key(__fn)?;
                    <$Tf as $crate::serde::Serialize>::serialize($f, $s)?;
                })+
                $s.end_map()?;
                $s.end_map()
            },]
            [$($udx)*]
            [$($ddx)* {
                let __n = stringify!($V); $(let __n = $rn;)?
                if $n == __n {
                    $(let mut $f: Option<$Tf> = None;)+
                    $d.begin_map()?;
                    while let Some(__k_owned) = $d.map_next()? {
                        let __k = __k_owned.as_str();
                        let mut __matched = false;
                        $({
                            let __fn = stringify!($f); $(let __fn = $frn;)?
                            if !__matched && __k == __fn {
                                $f = Some(<$Tf as $crate::serde::Deserialize<'de>>::deserialize($d)?);
                                __matched = true;
                            }
                        })+
                        if !__matched { $d.skip()?; }
                    }
                    return Ok(Self::$V {
                        $($f: $f.ok_or_else(|| $crate::serde::err(
                            concat!("missing field `", stringify!($f), "`")
                        ))?),+
                    });
                }
            }]
            $($rest)*
        );
    };
}
