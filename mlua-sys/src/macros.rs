#[allow(unused_macros)]
macro_rules! cstr {
    ($s:expr) => {
        concat!($s, "\0") as *const str as *const [::std::os::raw::c_char] as *const ::std::os::raw::c_char
    };
}

#[allow(unused_macros)]
macro_rules! propagate {
    ($L:expr $(, $ret:expr)?) => {
        #[cfg(feature = "luau-errors-as-returns")]
        {
            if $crate::luau::lua::lua_haserror($L) != 0 {
                return $($ret)?;
            }
        }
    };
}
