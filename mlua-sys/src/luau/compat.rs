//! MLua compatibility layer for Luau.
//!
//! Based on github.com/keplerproject/lua-compat-5.3

use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_void};
use std::{mem, ptr};

use super::lauxlib::*;
use super::lua::*;
#[cfg(not(feature = "luau-no-compiler"))]
use super::luacode::*;

pub const LUA_RESUMEERROR: c_int = -1;

// Keep in sync with Bytecode.h
const LBC_VERSION_MAX: u8 = 11;
const LBC_TYPE_VERSION_MIN: u8 = 1;
const LBC_TYPE_VERSION_MAX: u8 = 3;

unsafe fn compat53_reverse(L: *mut lua_State, mut a: c_int, mut b: c_int) {
    while a < b {
        lua_pushvalue(L, a);
        lua_pushvalue(L, b);
        lua_replace(L, a);
        lua_replace(L, b);
        a += 1;
        b -= 1;
    }
}

const COMPAT53_LEVELS1: c_int = 10; // size of the first part of the stack
const COMPAT53_LEVELS2: c_int = 11; // size of the second part of the stack

unsafe fn compat53_findfield(L: *mut lua_State, objidx: c_int, level: c_int) -> c_int {
    if level == 0 || lua_istable(L, -1) == 0 {
        return 0; // not found
    }

    lua_pushnil(L); // start 'next' loop
    while lua_next(L, -2) != 0 {
        // for each pair in table
        if lua_type(L, -2) == LUA_TSTRING {
            // ignore non-string keys
            if lua_rawequal(L, objidx, -1) != 0 {
                // found object?
                lua_pop(L, 1); // remove value (but keep name)
                return 1;
            } else if compat53_findfield(L, objidx, level - 1) != 0 {
                propagate!(L, 0);
                // stack: lib_name, lib_table, field_name (top)
                lua_pushliteral(L, c"."); // place '.' between the two names
                propagate!(L, 0);
                lua_replace(L, -3); // (in the slot occupied by table)
                lua_concat(L, 3); // lib_name.field_name
                propagate!(L, 0);
                return 1;
            }
        }
        propagate!(L, 0);
        lua_pop(L, 1); // remove value
    }
    0 // not found
}

unsafe fn compat53_pushglobalfuncname(
    L: *mut lua_State,
    L1: *mut lua_State,
    level: c_int,
    ar: *mut lua_Debug,
) -> c_int {
    let top = lua_gettop(L);
    lua_getinfo(L1, level, cstr!("f"), ar); // push function
    propagate!(L, 0);
    lua_xmove(L1, L, 1); // and move onto L
    lua_pushvalue(L, LUA_GLOBALSINDEX);
    luaL_checkstack(L, 6, cstr!("not enough stack")); // slots for 'findfield'
    propagate!(L, 0);
    let found = compat53_findfield(L, top + 1, 2);
    propagate!(L, 0);
    if found != 0 {
        let name = lua_tostring(L, -1);
        if CStr::from_ptr(name).to_bytes().starts_with(b"_G.") {
            lua_pushstring(L, name.add(3)); // push name without prefix
            propagate!(L, 0);
            lua_remove(L, -2); // remove original name
        }
        lua_copy(L, -1, top + 1); // move name to proper place
        lua_settop(L, top + 1); // remove pushed values
        1
    } else {
        lua_settop(L, top); // remove function and global table
        0
    }
}

unsafe fn compat53_pushfuncname(L: *mut lua_State, L1: *mut lua_State, level: c_int, ar: *mut lua_Debug) {
    if !(*ar).name.is_null() {
        // is there a name?
        lua_pushfstring(L, cstr!("function '%s'"), (*ar).name);
    } else if compat53_pushglobalfuncname(L, L1, level, ar) != 0 {
        lua_pushfstring(L, cstr!("function '%s'"), lua_tostring(L, -1));
        propagate!(L);
        lua_remove(L, -2); // remove name
    } else if lua_haserror(L) != 0 {
    } else if *(*ar).what != b'C' as c_char {
        // for Lua functions, use <file:line>
        lua_pushfstring(L, cstr!("function <%s:%d>"), (*ar).short_src, (*ar).linedefined);
    } else {
        lua_pushliteral(L, c"?");
    }
}

//
// lua ported functions
//

pub unsafe fn lua_rotate(L: *mut lua_State, mut idx: c_int, mut n: c_int) {
    idx = lua_absindex(L, idx);
    if n > 0 {
        // Faster version
        for _ in 0..n {
            lua_insert(L, idx);
        }
        return;
    }
    let n_elems = lua_gettop(L) - idx + 1;
    if n < 0 {
        n += n_elems;
    }
    if n > 0 && n < n_elems {
        luaL_checkstack(L, 2, cstr!("not enough stack slots available"));
        propagate!(L);
        n = n_elems - n;
        compat53_reverse(L, idx, idx + n - 1);
        compat53_reverse(L, idx + n, idx + n_elems - 1);
        compat53_reverse(L, idx, idx + n_elems - 1);
    }
}

#[inline(always)]
pub unsafe fn lua_copy(L: *mut lua_State, fromidx: c_int, toidx: c_int) {
    let abs_to = lua_absindex(L, toidx);
    luaL_checkstack(L, 1, cstr!("not enough stack slots available"));
    propagate!(L);
    lua_pushvalue(L, fromidx);
    lua_replace(L, abs_to);
}

#[inline(always)]
pub unsafe fn lua_isinteger(L: *mut lua_State, idx: c_int) -> c_int {
    if lua_type(L, idx) == LUA_TNUMBER {
        let n = lua_tonumber(L, idx);
        let i = lua_tointeger(L, idx);
        // Lua 5.3+ returns "false" for `-0.0`
        if n.to_bits() == (i as lua_Number).to_bits() {
            return 1;
        }
    }
    0
}

#[inline(always)]
pub unsafe fn lua_pushinteger(L: *mut lua_State, i: lua_Integer) {
    lua_pushnumber(L, i as lua_Number);
}

#[inline(always)]
pub unsafe fn lua_tointeger(L: *mut lua_State, i: c_int) -> lua_Integer {
    lua_tointegerx(L, i, ptr::null_mut())
}

pub unsafe fn lua_tointegerx(L: *mut lua_State, i: c_int, isnum: *mut c_int) -> lua_Integer {
    let mut ok = 0;
    let n = lua_tonumberx(L, i, &mut ok);
    let n_int = n as lua_Integer;
    if ok != 0
        && n >= lua_Integer::MIN as lua_Number
        && n < -(lua_Integer::MIN as lua_Number)
        && n == n_int as lua_Number
    {
        if !isnum.is_null() {
            *isnum = 1;
        }
        return n_int;
    }
    if !isnum.is_null() {
        *isnum = 0;
    }
    0
}

#[inline(always)]
pub unsafe fn lua_rawlen(L: *mut lua_State, idx: c_int) -> usize {
    lua_objlen(L, idx)
}

#[inline(always)]
pub unsafe fn lua_pushlstring(L: *mut lua_State, s: *const c_char, l: usize) -> *const c_char {
    if l == 0 {
        lua_pushlstring_(L, cstr!(""), 0);
    } else {
        lua_pushlstring_(L, s, l);
    }
    lua_tostring(L, -1)
}

#[inline(always)]
pub unsafe fn lua_pushstring(L: *mut lua_State, s: *const c_char) -> *const c_char {
    lua_pushstring_(L, s);
    lua_tostring(L, -1)
}

#[inline(always)]
pub unsafe fn lua_geti(L: *mut lua_State, mut idx: c_int, n: lua_Integer) -> c_int {
    idx = lua_absindex(L, idx);
    lua_pushinteger(L, n);
    lua_gettable(L, idx)
}

#[inline(always)]
pub unsafe fn lua_rawgeti(L: *mut lua_State, idx: c_int, n: lua_Integer) -> c_int {
    let n = n.try_into().expect("cannot convert index from lua_Integer");
    lua_rawgeti_(L, idx, n)
}

#[inline(always)]
pub unsafe fn lua_rawgetp(L: *mut lua_State, idx: c_int, p: *const c_void) -> c_int {
    lua_rawgetptagged(L, idx, p, 0)
}

#[inline(always)]
pub unsafe fn lua_getuservalue(L: *mut lua_State, mut idx: c_int) -> c_int {
    luaL_checkstack(L, 2, cstr!("not enough stack slots available"));
    propagate!(L, LUA_TNIL);
    idx = lua_absindex(L, idx);
    lua_pushliteral(L, c"__mlua_uservalues");
    propagate!(L, LUA_TNIL);
    if lua_rawget(L, LUA_REGISTRYINDEX) != LUA_TTABLE {
        return LUA_TNIL;
    }
    lua_pushvalue(L, idx);
    lua_rawget(L, -2);
    lua_remove(L, -2);
    lua_type(L, -1)
}

#[inline(always)]
pub unsafe fn lua_seti(L: *mut lua_State, mut idx: c_int, n: lua_Integer) {
    luaL_checkstack(L, 1, cstr!("not enough stack slots available"));
    propagate!(L);
    idx = lua_absindex(L, idx);
    lua_pushinteger(L, n);
    lua_insert(L, -2);
    lua_settable(L, idx);
}

#[inline(always)]
pub unsafe fn lua_rawseti(L: *mut lua_State, idx: c_int, n: lua_Integer) {
    let n = n.try_into().expect("cannot convert index from lua_Integer");
    lua_rawseti_(L, idx, n)
}

#[inline(always)]
pub unsafe fn lua_rawsetp(L: *mut lua_State, idx: c_int, p: *const c_void) {
    lua_rawsetptagged(L, idx, p, 0)
}

#[inline(always)]
pub unsafe fn lua_setuservalue(L: *mut lua_State, mut idx: c_int) {
    luaL_checkstack(L, 4, cstr!("not enough stack slots available"));
    propagate!(L);
    idx = lua_absindex(L, idx);
    lua_pushliteral(L, c"__mlua_uservalues");
    propagate!(L);
    lua_pushvalue(L, -1);
    if lua_rawget(L, LUA_REGISTRYINDEX) != LUA_TTABLE {
        lua_pop(L, 1);
        lua_createtable(L, 0, 2); // main table
        propagate!(L);
        lua_createtable(L, 0, 1); // metatable
        propagate!(L);
        lua_pushliteral(L, c"k");
        propagate!(L);
        lua_setfield(L, -2, cstr!("__mode"));
        propagate!(L);
        lua_setmetatable(L, -2);
        lua_pushvalue(L, -2);
        lua_pushvalue(L, -2);
        lua_rawset(L, LUA_REGISTRYINDEX);
        propagate!(L);
    }
    lua_replace(L, -2);
    lua_pushvalue(L, idx);
    lua_pushvalue(L, -3);
    lua_remove(L, -4);
    lua_rawset(L, -3);
    propagate!(L);
    lua_pop(L, 1);
}

#[inline(always)]
pub unsafe fn lua_len(L: *mut lua_State, idx: c_int) {
    match lua_type(L, idx) {
        LUA_TSTRING => {
            lua_pushnumber(L, lua_objlen(L, idx) as lua_Number);
        }
        LUA_TTABLE => {
            let called = luaL_callmeta(L, idx, cstr!("__len"));
            propagate!(L);
            if called == 0 {
                lua_pushnumber(L, lua_objlen(L, idx) as lua_Number);
            }
        }
        LUA_TUSERDATA if luaL_callmeta(L, idx, cstr!("__len")) != 0 => {}
        _ => {
            propagate!(L);
            luaL_raiseerror(
                L,
                cstr!("attempt to get length of a %s value"),
                lua_typename(L, lua_type(L, idx)),
            );
        }
    }
}

#[inline(always)]
pub unsafe fn lua_pushglobaltable(L: *mut lua_State) {
    lua_pushvalue(L, LUA_GLOBALSINDEX);
}

#[inline(always)]
pub unsafe fn lua_resume(L: *mut lua_State, from: *mut lua_State, narg: c_int, nres: *mut c_int) -> c_int {
    let ret = lua_resume_(L, from, narg);
    if (ret == LUA_OK || ret == LUA_YIELD) && !(nres.is_null()) {
        *nres = lua_gettop(L);
    }
    ret
}

#[inline(always)]
pub unsafe fn lua_resumex(L: *mut lua_State, from: *mut lua_State, narg: c_int, nres: *mut c_int) -> c_int {
    let ret = if narg == LUA_RESUMEERROR {
        lua_resumeerror(L, from)
    } else {
        lua_resume_(L, from, narg)
    };
    if (ret == LUA_OK || ret == LUA_YIELD) && !(nres.is_null()) {
        *nres = lua_gettop(L);
    }
    ret
}

//
// lauxlib ported functions
//

#[inline(always)]
pub unsafe fn luaL_checkstack(L: *mut lua_State, sz: c_int, msg: *const c_char) {
    if lua_checkstack(L, sz + LUA_MINSTACK) == 0 {
        if !msg.is_null() {
            luaL_raiseerror(L, cstr!("stack overflow (%s)"), msg);
        } else {
            lua_pushliteral(L, c"stack overflow");
            propagate!(L);
            lua_raiseerror(L);
        }
    }
}

pub unsafe fn luaL_requireinteger(L: *mut lua_State, narg: c_int, out: *mut lua_Integer) -> c_int {
    let mut isnum = 0;
    *out = lua_tointegerx(L, narg, &mut isnum);
    if isnum == 0 {
        luaL_raisetypeerror(L, narg, lua_typename(L, LUA_TNUMBER));
        return 0;
    }
    1
}

pub unsafe fn luaL_requireoptinteger(
    L: *mut lua_State,
    narg: c_int,
    def: lua_Integer,
    out: *mut lua_Integer,
) -> c_int {
    if lua_isnoneornil(L, narg) != 0 {
        *out = def;
        1
    } else {
        luaL_requireinteger(L, narg, out)
    }
}

#[cfg(not(feature = "luau-errors-as-returns"))]
#[inline(always)]
pub unsafe fn luaL_checkinteger(L: *mut lua_State, narg: c_int) -> lua_Integer {
    let mut isnum = 0;
    let int = lua_tointegerx(L, narg, &mut isnum);
    if isnum == 0 {
        luaL_typeerror(L, narg, lua_typename(L, LUA_TNUMBER));
    }
    int
}

#[cfg(not(feature = "luau-errors-as-returns"))]
pub unsafe fn luaL_optinteger(L: *mut lua_State, narg: c_int, def: lua_Integer) -> lua_Integer {
    if lua_isnoneornil(L, narg) != 0 {
        def
    } else {
        luaL_checkinteger(L, narg)
    }
}

#[inline(always)]
pub unsafe fn luaL_getmetafield(L: *mut lua_State, obj: c_int, e: *const c_char) -> c_int {
    if luaL_getmetafield_(L, obj, e) != 0 {
        lua_type(L, -1)
    } else {
        LUA_TNIL
    }
}

#[inline(always)]
pub unsafe fn luaL_newmetatable(L: *mut lua_State, tname: *const c_char) -> c_int {
    let created = luaL_newmetatable_(L, tname);
    propagate!(L, 0);
    if created != 0 {
        lua_pushstring(L, tname);
        propagate!(L, 0);
        lua_setfield(L, -2, cstr!("__type"));
        propagate!(L, 0);
        1
    } else {
        0
    }
}

// Detects whether a chunk is Luau bytecode or text source.
pub unsafe fn luaL_isbytecode(data: *const c_char, size: usize) -> bool {
    if size == 0 {
        return false;
    }
    match *data as u8 {
        b if b < b'\t' => true, // bytecode
        b if b <= LBC_VERSION_MAX => {
            let types_version = (size >= 2).then(|| *data.add(1) as u8);
            match types_version {
                Some(LBC_TYPE_VERSION_MIN..=LBC_TYPE_VERSION_MAX) => true, // bytecode
                _ => false,                                                // text
            }
        }
        _ => false, // text
    }
}

#[cfg_attr(feature = "luau-no-compiler", allow(unused_mut))]
pub unsafe fn luaL_loadbufferenv(
    L: *mut lua_State,
    data: *const c_char,
    mut size: usize,
    name: *const c_char,
    mode: *const c_char,
    mut env: c_int,
) -> c_int {
    #[cfg(not(feature = "luau-no-compiler"))]
    unsafe extern "C" {
        fn free(p: *mut c_void);
    }

    #[cfg(not(feature = "luau-no-compiler"))]
    unsafe extern "C" fn data_dtor(_: *mut lua_State, data: *mut c_void) {
        free(*(data as *mut *mut c_char) as *mut c_void);
    }

    let is_bytecode = luaL_isbytecode(data, size);
    if !mode.is_null() {
        let modeb = CStr::from_ptr(mode).to_bytes();
        let allow_binary = modeb.contains(&b'b');
        let allow_text = modeb.contains(&b't');
        if is_bytecode && !allow_binary {
            lua_pushfstring(L, cstr!("attempt to load a binary chunk (mode is '%s')"), mode);
            return LUA_ERRSYNTAX;
        } else if !is_bytecode && !allow_text {
            lua_pushfstring(L, cstr!("attempt to load a text chunk (mode is '%s')"), mode);
            return LUA_ERRSYNTAX;
        }
    }

    #[cfg(feature = "luau-no-compiler")]
    let status = {
        if !is_bytecode {
            lua_pushliteral(L, c"attempt to load a text chunk (compiler is not available)");
            propagate!(L, LUA_ERRMEM);
            return LUA_ERRSYNTAX;
        }
        let status = luau_load(L, name, data, size, env);
        propagate!(L, LUA_ERRMEM);
        status
    };

    #[cfg(not(feature = "luau-no-compiler"))]
    let status = if !is_bytecode {
        if env < 0 {
            env -= 1;
        }
        let data_ud = lua_newuserdatadtor(L, mem::size_of::<*mut c_char>(), data_dtor) as *mut *mut c_char;
        propagate!(L, LUA_ERRMEM);
        let data = luau_compile_(data, size, ptr::null_mut(), &mut size);
        ptr::write(data_ud, data);
        // By deferring the `free(data)` to the userdata destructor, we ensure that
        // even if `luau_load` throws an error, the `data` is still released.
        let status = luau_load(L, name, data, size, env);
        propagate!(L, LUA_ERRMEM);
        lua_replace(L, -2); // replace data with the result
        status
    } else {
        let status = luau_load(L, name, data, size, env);
        propagate!(L, LUA_ERRMEM);
        status
    };

    if status != 0 {
        if lua_isstring(L, -1) != 0 && CStr::from_ptr(lua_tostring(L, -1)) == c"not enough memory" {
            // A case for Luau >= 0.679
            return LUA_ERRMEM;
        }
        return LUA_ERRSYNTAX;
    }

    LUA_OK
}

#[inline(always)]
pub unsafe fn luaL_loadbufferx(
    L: *mut lua_State,
    data: *const c_char,
    size: usize,
    name: *const c_char,
    mode: *const c_char,
) -> c_int {
    luaL_loadbufferenv(L, data, size, name, mode, 0)
}

#[inline(always)]
pub unsafe fn luaL_loadbuffer(
    L: *mut lua_State,
    data: *const c_char,
    size: usize,
    name: *const c_char,
) -> c_int {
    luaL_loadbufferenv(L, data, size, name, ptr::null(), 0)
}

#[inline(always)]
pub unsafe fn luaL_len(L: *mut lua_State, idx: c_int) -> lua_Integer {
    let mut isnum = 0;
    luaL_checkstack(L, 1, cstr!("not enough stack slots available"));
    propagate!(L, 0);
    lua_len(L, idx);
    propagate!(L, 0);
    let res = lua_tointegerx(L, -1, &mut isnum);
    lua_pop(L, 1);
    if isnum == 0 {
        luaL_raiseerror(L, cstr!("object length is not an integer"));
        return 0;
    }
    res
}

pub unsafe fn luaL_traceback(L: *mut lua_State, L1: *mut lua_State, msg: *const c_char, mut level: c_int) {
    let mut ar: lua_Debug = mem::zeroed();
    let numlevels = lua_stackdepth(L);
    #[rustfmt::skip]
    let mut limit = if numlevels - level > COMPAT53_LEVELS1 + COMPAT53_LEVELS2 { COMPAT53_LEVELS1 } else { -1 };

    let mut buf: luaL_Strbuf = mem::zeroed();
    luaL_buffinit(L, &mut buf);
    propagate!(L);

    if !msg.is_null() {
        luaL_addstring(&mut buf, msg);
        luaL_addstring(&mut buf, cstr!("\n"));
    }
    luaL_addstring(&mut buf, cstr!("stack traceback:"));
    propagate!(L);
    while lua_getinfo(L1, level, cstr!("sln"), &mut ar) != 0 {
        propagate!(L);
        if limit == 0 {
            // too many levels?
            let n = numlevels - level - COMPAT53_LEVELS2;
            // add warning about skip ("n + 1" because we skip current level too)
            lua_pushfstring(L, cstr!("\n\t...\t(skipping %d levels)"), n + 1);
            propagate!(L);
            luaL_addvalue(&mut buf);
            level += n; // and skip to last levels
        } else {
            luaL_addstring(&mut buf, cstr!("\n\t"));
            luaL_addstring(&mut buf, ar.short_src);
            luaL_addstring(&mut buf, cstr!(":"));
            if ar.currentline > 0 {
                luaL_addunsigned(&mut buf, ar.currentline as _);
                luaL_addstring(&mut buf, cstr!(":"));
            }
            luaL_addstring(&mut buf, cstr!(" in "));
            compat53_pushfuncname(L, L1, level, &mut ar);
            propagate!(L);
            luaL_addvalue(&mut buf);
        }
        propagate!(L);
        level += 1;
        limit -= 1;
    }
    propagate!(L);
    luaL_pushresult(&mut buf);
}

pub unsafe fn luaL_tolstring(L: *mut lua_State, mut idx: c_int, len: *mut usize) -> *const c_char {
    idx = lua_absindex(L, idx);
    let called = luaL_callmeta(L, idx, cstr!("__tostring"));
    propagate!(L, ptr::null());
    if called == 0 {
        match lua_type(L, idx) {
            LUA_TNIL => {
                lua_pushliteral(L, c"nil");
            }
            LUA_TSTRING | LUA_TNUMBER => {
                lua_pushvalue(L, idx);
            }
            LUA_TBOOLEAN => {
                if lua_toboolean(L, idx) == 0 {
                    lua_pushliteral(L, c"false");
                } else {
                    lua_pushliteral(L, c"true");
                }
            }
            t => {
                let tt = luaL_getmetafield(L, idx, cstr!("__type"));
                propagate!(L, ptr::null());
                let name = if tt == LUA_TSTRING {
                    lua_tostring(L, -1)
                } else {
                    lua_typename(L, t)
                };
                lua_pushfstring(L, cstr!("%s: %p"), name, lua_topointer(L, idx));
                propagate!(L, ptr::null());
                if tt != LUA_TNIL {
                    lua_replace(L, -2); // remove '__type'
                }
            }
        };
        propagate!(L, ptr::null());
    } else if lua_isstring(L, -1) == 0 {
        luaL_raiseerror(L, cstr!("'__tostring' must return a string"));
        return ptr::null();
    }
    lua_tolstring(L, -1, len)
}

#[inline(always)]
pub unsafe fn luaL_setmetatable(L: *mut lua_State, tname: *const c_char) {
    luaL_checkstack(L, 1, cstr!("not enough stack slots available"));
    propagate!(L);
    luaL_getmetatable(L, tname);
    propagate!(L);
    lua_setmetatable(L, -2);
}

pub unsafe fn luaL_getsubtable(L: *mut lua_State, idx: c_int, fname: *const c_char) -> c_int {
    let abs_i = lua_absindex(L, idx);
    luaL_checkstack(L, 3, cstr!("not enough stack slots available"));
    propagate!(L, 0);
    lua_pushstring_(L, fname);
    propagate!(L, 0);
    let t = lua_gettable(L, abs_i);
    propagate!(L, 0);
    if t == LUA_TTABLE {
        return 1;
    }
    lua_pop(L, 1);
    lua_newtable(L);
    propagate!(L, 0);
    lua_pushstring_(L, fname);
    propagate!(L, 0);
    lua_pushvalue(L, -2);
    lua_settable(L, abs_i);
    0
}

pub unsafe fn luaL_requiref(L: *mut lua_State, modname: *const c_char, openf: lua_CFunction, glb: c_int) {
    luaL_checkstack(L, 3, cstr!("not enough stack slots available"));
    propagate!(L);
    luaL_getsubtable(L, LUA_REGISTRYINDEX, LUA_LOADED_TABLE);
    propagate!(L);
    let t = lua_getfield(L, -1, modname);
    propagate!(L);
    if t == LUA_TNIL {
        lua_pop(L, 1);
        lua_pushcfunction(L, openf);
        propagate!(L);
        lua_pushstring(L, modname);
        propagate!(L);
        lua_call(L, 1, 1);
        propagate!(L);
        lua_pushvalue(L, -1);
        lua_setfield(L, -3, modname);
        propagate!(L);
    }
    if glb != 0 {
        lua_pushvalue(L, -1);
    } else {
        lua_pushnil(L);
    }
    lua_setglobal(L, modname);
    propagate!(L);
    lua_replace(L, -2);
}
