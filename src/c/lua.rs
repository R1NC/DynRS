use crate::c::util::{box_into_raw_new, cstr_to_rust, rust_to_cstr};
use crate::core::lua::LuaBridge;
use std::ffi::{c_char, c_void};

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_lua_bridge_init() -> *mut c_void {
    match LuaBridge::new() {
        Ok(bridge) => box_into_raw_new(bridge) as *mut c_void,
        Err(_) => std::ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_lua_bridge_release(bridge: *mut c_void) {
    if !bridge.is_null() {
        unsafe { drop(Box::from_raw(bridge as *mut LuaBridge)) };
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_lua_load_file(bridge: *mut c_void, path: *const c_char) -> bool {
    if bridge.is_null() || path.is_null() {
        return false;
    }
    let bridge = unsafe { &*(bridge as *mut LuaBridge) };
    let path_str = match cstr_to_rust(path) {
        Some(s) => s,
        None => return false,
    };
    bridge.load_file(path_str).is_ok()
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_lua_load_string(bridge: *mut c_void, script: *const c_char) -> bool {
    if bridge.is_null() || script.is_null() {
        return false;
    }
    let bridge = unsafe { &*(bridge as *mut LuaBridge) };
    let script_str = match cstr_to_rust(script) {
        Some(s) => s,
        None => return false,
    };
    bridge.load_string(script_str).is_ok()
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_lua_call_function(
    bridge: *mut c_void,
    func_name: *const c_char,
    arg: *const c_char,
    result_out: *mut *mut c_char,
    err_out: *mut *mut c_char,
) -> bool {
    if bridge.is_null() || func_name.is_null() {
        return false;
    }

    let bridge = unsafe { &*(bridge as *mut LuaBridge) };
    let func_name_str = match cstr_to_rust(func_name) {
        Some(s) => s,
        None => return false,
    };

    let arg_str = match cstr_to_rust(arg) {
        Some(s) => s,
        None => return false,
    };

    match bridge.call_function(func_name_str, arg_str) {
        Ok(result) => {
            if !result_out.is_null() {
                unsafe { *result_out = rust_to_cstr(result) };
            }
            true
        }
        Err(e) => {
            if !err_out.is_null() {
                unsafe { *err_out = rust_to_cstr(e.to_string()) };
            }
            false
        }
    }
}
