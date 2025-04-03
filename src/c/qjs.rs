use crate::c::util::{box_into_raw_new, cstr_to_rust, cbytes_to_rust, ngenrs_free_ptr, rust_to_cstr};
use crate::core::qjs::JSBridge;
use libc::{c_char, c_void};

/// Creates a new JSBridge instance
#[unsafe(no_mangle)]
pub extern "C" 
fn ngenrs_jsbridge_init() -> *mut c_void {
    box_into_raw_new(JSBridge::new()) as *mut c_void
}

/// Loads a script from file
#[unsafe(no_mangle)]
pub extern "C" 
fn ngenrs_jsbridge_load_script_file(
    handle: *mut c_void,
    path: *const c_char,
    is_module: bool,
    err_out: *mut *mut c_char,
) -> bool {
    if handle.is_null() || path.is_null() {
        return false;
    }

    let bridge = unsafe { &*(handle as *mut JSBridge) };
    let path_str = match cstr_to_rust(path) {
        Some(s) => s,
        None => return false,
    };

    match bridge.load_script_file(path_str, is_module) {
        Ok(_) => true,
        Err(e) => {
            if !err_out.is_null() {
                unsafe { *err_out = rust_to_cstr(e) };
            }
            false
        }
    }
}

/// Loads a script from string
#[unsafe(no_mangle)]
pub extern "C" 
fn ngenrs_qjs_load_script_content(
    handle: *mut c_void,
    script: *const c_char,
    is_module: bool,
    err_out: *mut *mut c_char,
) -> bool {
    if handle.is_null() || script.is_null() {
        return false;
    }

    let bridge = unsafe { &*(handle as *mut JSBridge) };
    let script_str = match cstr_to_rust(script) {
        Some(s) => s,
        None => return false,
    };

    match bridge.load_script_content(script_str, is_module) {
        Ok(_) => true,
        Err(e) => {
            if !err_out.is_null() {
                unsafe { *err_out = rust_to_cstr(e) };
            }
            false
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" 
fn ngenrs_jsbridge_load_bytecode_file(
    handle: *mut c_void,
    path: *const c_char,
    err_out: *mut *mut c_char,
) -> bool {
    if handle.is_null() || path.is_null() {
        return false;
    }

    unsafe {
        let bridge = &*(handle as *mut JSBridge);
        let path_str = match cstr_to_rust(path) {
            Some(s) => s,
            None => return false,
        };

        match bridge.load_bytecode_file(path_str) {
            Ok(_) => true,
            Err(e) => {
                if !err_out.is_null() {
                    *err_out = rust_to_cstr(e);
                }
                false
            }
        }
    }
}

/// Loads bytecode from memory buffer
#[unsafe(no_mangle)]
pub extern "C" 
fn ngenrs_jsbridge_load_bytecode_content(
    handle: *mut c_void,
    bytecode: *const u8,
    length: usize,
    err_out: *mut *mut c_char,
) -> bool {
    if handle.is_null() || bytecode.is_null() {
        return false;
    }

    unsafe {
        let bridge = &*(handle as *mut JSBridge);
        let bytecode_slice = match cbytes_to_rust(bytecode, length) {
            Some(slice) => slice,
            None => return false,
        };

        match bridge.load_bytecode_content(bytecode_slice) {
            Ok(_) => true,
            Err(e) => {
                if !err_out.is_null() {
                    *err_out = rust_to_cstr(e);
                }
                false
            }
        }
    }
}

/// Calls a JavaScript function with single string argument
#[unsafe(no_mangle)]
pub extern "C" 
fn ngenrs_jsbridge_call_function(
    handle: *mut c_void,
    func_name: *const c_char,
    arg: *const c_char,
    result_out: *mut *mut c_char,
    err_out: *mut *mut c_char,
) -> bool {
    if handle.is_null() || func_name.is_null() {
        return false;
    }

    let bridge = unsafe { &*(handle as *mut JSBridge) };
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
                unsafe { *err_out = rust_to_cstr(e) };
            }
            false
        }
    }
}

/// Frees a JSBridge instance
#[unsafe(no_mangle)]
pub extern "C" 
fn ngenrs_jsbridge_release(handle: *mut c_void) {
    if !handle.is_null() {
        ngenrs_free_ptr(handle as *mut JSBridge);
    }
}
