use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use libc;
use std::slice;

/// Utility function to convert C string to Rust string (safe wrapper)
pub fn cstr_to_rust(cstr: *const c_char) -> Option<&'static str> {
    if cstr.is_null() {
        return None;
    }
    unsafe {
        let len = libc::strlen(cstr);
        if len > isize::MAX as usize {
            return None;
        }
        CStr::from_ptr(cstr).to_str().ok()
    }
}

/// Utility function to convert Rust string to C string (transfers ownership)
pub fn rust_to_cstr(rstr: String) -> *mut c_char {
    match CString::new(rstr) {
        Ok(cstr) => cstr.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Converts C byte array to Rust slice (safe wrapper)
pub fn cbytes_to_rust(data: *const u8, len: usize) -> Option<&'static [u8]> {
    if data.is_null() {
        None
    } else {
        unsafe {
            if len <= isize::MAX as usize {
                Some(slice::from_raw_parts(data, len))
            } else {
                None
            }
        }
    }
}

/// Converts Rust Vec<u8> to C-owned bytes (transfers ownership)
pub fn rust_to_cbytes(data: Vec<u8>) -> (*mut u8, usize) {
    let boxed = data.into_boxed_slice();
    let len = boxed.len();
    (Box::into_raw(boxed) as *mut u8, len)
}

pub fn free<T>(_x: T) {
    drop(_x);
}

pub fn ngenrs_free_ptr<T>(raw: *mut T) {
    if !raw.is_null() {
        unsafe { free(Box::from_raw(raw)) };
    }
}

#[unsafe(no_mangle)]
pub extern "C"
fn ngenrs_free_cstr(s: *mut c_char) {
    free(unsafe { CString::from_raw(s) });
}

#[unsafe(no_mangle)]
pub extern "C"
fn ngenrs_free_bytes(buf: *mut u8, len: usize) {
    ngenrs_free_ptr(unsafe { slice::from_raw_parts_mut(buf, len).as_mut_ptr() });
}

pub fn box_into_raw_new<T>(value: T) -> *mut T {
    Box::into_raw(Box::new(value))
}

pub unsafe fn rust_map_from_c_arrays(
    keys: *const *const c_char,
    values: *const *const c_char,
    len: usize,
) -> Option<HashMap<String, String>> {
    if keys.is_null() || values.is_null() {
        return None;
    }
    let mut map = HashMap::new();
    let keys_slice = unsafe { std::slice::from_raw_parts(keys, len) };
    let values_slice = unsafe { std::slice::from_raw_parts(values, len) };
    
    for i in 0..len {
        if let (Some(key), Some(value)) = (cstr_to_rust(keys_slice[i]), cstr_to_rust(values_slice[i])) {
            map.insert(key.to_string(), value.to_string());
        }
    }
    Some(map)
}

/// Converts Rust HashMap to C-style string arrays
/// Returns tuple of (keys_ptr, values_ptr, len)
pub unsafe fn rust_map_to_c_arrays(
    map: &HashMap<String, String>,
    keys_out: *mut *mut c_char,
    values_out: *mut *mut c_char,
    count_out: *mut usize
) {
    if keys_out.is_null() || values_out.is_null() || count_out.is_null() {
        return;
    }

    let len = map.len();
    unsafe { *count_out = len };

    if len == 0 {
        return;
    }

    // Allocate arrays for keys and values
    let mut keys_vec: Vec<*mut c_char> = Vec::with_capacity(len);
    let mut values_vec: Vec<*mut c_char> = Vec::with_capacity(len);

    for (k, v) in map {
        keys_vec.push(rust_to_cstr(k.clone()));
        values_vec.push(rust_to_cstr(v.clone()));
    }

    let keys_ptr = Box::into_raw(keys_vec.into_boxed_slice()) as *mut *mut c_char;
    let values_ptr = Box::into_raw(values_vec.into_boxed_slice()) as *mut *mut c_char;
    
    // Copy to output pointers
    unsafe {
        std::ptr::copy_nonoverlapping(keys_ptr, keys_out, len);
        std::ptr::copy_nonoverlapping(values_ptr, values_out, len);
        
        ngenrs_free_ptr(keys_ptr);
        ngenrs_free_ptr(values_ptr);
    }
}