#[macro_export]
macro_rules! thiserror {
    ($(#[$m:meta])* $vis:vis enum $Name:ident { $($body:tt)* }) => {
        $crate::__thiserror_go!(
            ($(#[$m])*) $vis $Name [] [] [] []
            $($body)* ,
        );
    };
}

#[macro_export]
macro_rules! __thiserror_go {
    (($($m:tt)*) $vis:vis $Name:ident
     [$($variants:tt)*]
     [$({$($pat:tt)*}{$kind:tt $($disp_arg:tt)*})*]
     [$($source:tt)*]
     [$($froms:tt)*]
     $(,)?
    ) => {
        $($m)*
        #[derive(Debug)]
        $vis enum $Name { $($variants)* }

        impl ::std::fmt::Display for $Name {
            #[allow(unused_variables)]
            fn fmt(&self, __f: &mut ::std::fmt::Formatter)
                -> ::std::fmt::Result
            {
                match self {
                    $($($pat)* => $crate::__te_disp!(__f, $kind $($disp_arg)*),)*
                }
            }
        }

        impl ::std::error::Error for $Name {
            #[allow(unreachable_patterns, unused_variables)]
            fn source(&self)
                -> ::std::option::Option<&(dyn ::std::error::Error + 'static)>
            {
                match self {
                    $($source)*
                    _ => None,
                }
            }
        }

        $($froms)*
    };

    (($($m:tt)*) $vis:vis $Name:ident
     [$($variants:tt)*] [$($disp:tt)*] [$($source:tt)*] [$($froms:tt)*]
     $fmt:literal $V:ident , $($rest:tt)*
    ) => {
        $crate::__thiserror_go!(($($m)*) $vis $Name
            [$($variants)* $V,]
            [$($disp)* {Self::$V}{fmt $fmt}]
            [$($source)*]
            [$($froms)*]
            $($rest)*);
    };

    (($($m:tt)*) $vis:vis $Name:ident
     [$($variants:tt)*] [$($disp:tt)*] [$($source:tt)*] [$($froms:tt)*]
     transparent $V:ident ( $T:ty ) , $($rest:tt)*
    ) => {
        $crate::__thiserror_go!(($($m)*) $vis $Name
            [$($variants)* $V($T),]
            [$($disp)* {Self::$V(__0)}{transparent __0}]
            [$($source)* Self::$V(__e) => __e.source(),]
            [$($froms)*]
            $($rest)*);
    };

    (($($m:tt)*) $vis:vis $Name:ident
     [$($variants:tt)*] [$($disp:tt)*] [$($source:tt)*] [$($froms:tt)*]
     $fmt:literal $V:ident (from $T:ty) , $($rest:tt)*
    ) => {
        $crate::__thiserror_go!(($($m)*) $vis $Name
            [$($variants)* $V($T),]
            [$($disp)* {Self::$V(__0)}{fmt $fmt, __0}]
            [$($source)* Self::$V(__e) => Some(__e),]
            [$($froms)*
                impl ::std::convert::From<$T> for $Name {
                    fn from(__e: $T) -> Self { Self::$V(__e) }
                }
            ]
            $($rest)*);
    };

    (($($m:tt)*) $vis:vis $Name:ident
     [$($variants:tt)*] [$($disp:tt)*] [$($source:tt)*] [$($froms:tt)*]
     $fmt:literal $V:ident (source $T:ty) , $($rest:tt)*
    ) => {
        $crate::__thiserror_go!(($($m)*) $vis $Name
            [$($variants)* $V($T),]
            [$($disp)* {Self::$V(__0)}{fmt $fmt, __0}]
            [$($source)* Self::$V(__e) => Some(__e),]
            [$($froms)*]
            $($rest)*);
    };

    (($($m:tt)*) $vis:vis $Name:ident
     [$($variants:tt)*] [$($disp:tt)*] [$($source:tt)*] [$($froms:tt)*]
     $fmt:literal $V:ident ( $T:ty ) , $($rest:tt)*
    ) => {
        $crate::__thiserror_go!(($($m)*) $vis $Name
            [$($variants)* $V($T),]
            [$($disp)* {Self::$V(__0)}{fmt $fmt, __0}]
            [$($source)*]
            [$($froms)*]
            $($rest)*);
    };

    (($($m:tt)*) $vis:vis $Name:ident
     [$($variants:tt)*] [$($disp:tt)*] [$($source:tt)*] [$($froms:tt)*]
     $fmt:literal $V:ident ( $T0:ty, $T1:ty $(,)? ) , $($rest:tt)*
    ) => {
        $crate::__thiserror_go!(($($m)*) $vis $Name
            [$($variants)* $V($T0, $T1),]
            [$($disp)* {Self::$V(__0, __1)}{fmt $fmt, __0, __1}]
            [$($source)*]
            [$($froms)*]
            $($rest)*);
    };

    (($($m:tt)*) $vis:vis $Name:ident
     [$($variants:tt)*] [$($disp:tt)*] [$($source:tt)*] [$($froms:tt)*]
     $fmt:literal $V:ident ( $T0:ty, $T1:ty, $T2:ty $(,)? ) , $($rest:tt)*
    ) => {
        $crate::__thiserror_go!(($($m)*) $vis $Name
            [$($variants)* $V($T0, $T1, $T2),]
            [$($disp)* {Self::$V(__0, __1, __2)}{fmt $fmt, __0, __1, __2}]
            [$($source)*]
            [$($froms)*]
            $($rest)*);
    };

    (($($m:tt)*) $vis:vis $Name:ident
     [$($variants:tt)*] [$($disp:tt)*] [$($source:tt)*] [$($froms:tt)*]
     $fmt:literal $V:ident { $($f:ident : $T:ty),+ $(,)? } , $($rest:tt)*
    ) => {
        $crate::__thiserror_go!(($($m)*) $vis $Name
            [$($variants)* $V { $($f: $T),+ },]
            [$($disp)* {Self::$V { $($f),+ }}{fmt $fmt}]
            [$($source)*]
            [$($froms)*]
            $($rest)*);
    };
}

#[macro_export]
macro_rules! __te_disp {
    ($f:ident, fmt $fmt:literal $(, $arg:expr)*) => {
        write!($f, $fmt $(, $arg)*)
    };
    ($f:ident, transparent $inner:expr) => {
        ::std::fmt::Display::fmt($inner, $f)
    };
}
