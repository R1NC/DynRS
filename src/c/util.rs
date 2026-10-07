use libc;
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
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

/// Converts a Rust string into a C string this library owns.
///
/// `Err` when the text holds an interior NUL, because a C string cannot carry one. The caller has to
/// decide what that means: it used to be a silent null pointer, which is how a `redb` key containing
/// a NUL became a null entry in a key array and was lost without a word.
pub fn rust_to_cstr(rstr: String) -> Result<*mut c_char, std::ffi::NulError> {
    CString::new(rstr).map(CString::into_raw)
}

/// The same conversion for a value that is *meant* to be text, falling back to a fixed message when
/// it is not. Used for error text on its way into a script, where dropping the message would leave
/// the script with `undefined` instead of the reason.
pub fn rust_to_cstr_lossy(rstr: String) -> *mut c_char {
    match rust_to_cstr(rstr) {
        Ok(ptr) => ptr,
        Err(_) => CString::new("the message contained a NUL byte")
            .expect("the literal has no interior NUL")
            .into_raw(),
    }
}

/// Writes a Rust value's bytes into a C-owned buffer and its length through `len_out`.
///
/// This is the shape every entry point that returns *data* uses, because a value the caller or a
/// script produced may contain a NUL byte. As a C string such a value came back as a null pointer,
/// which is the same thing "there is no value" and "the call failed" report; with a length, a value
/// is reported whole and `NULL` means only "no value here".
///
/// An empty value still reports a real allocation of length zero, so it stays distinguishable from
/// `NULL`. Release a non-null result with `ngenrs_free_bytes(ptr)`.
pub fn bytes_to_c(data: Vec<u8>, len_out: *mut usize) -> *mut u8 {
    let (ptr, len) = rust_to_cbytes(data);
    if !len_out.is_null() {
        unsafe { *len_out = len };
    }
    ptr
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

/// Converts Rust `Vec<u8>` into a C-owned buffer (transfers ownership), returning the pointer and
/// the length the caller should report to its own caller.
///
/// The allocation carries its length in front of the data, so [`ngenrs_free_bytes`] can release it
/// from the pointer alone: a caller that passes back a length it no longer remembers — or one it
/// computed wrong — cannot make the deallocation use the wrong layout.
///
/// Every call produces a real allocation, even for an empty `data`, so the pointer is never null
/// and `NULL` can be reserved for "no value" at the entry points that return one.
pub fn rust_to_cbytes(data: Vec<u8>) -> (*mut u8, usize) {
    let len = data.len();
    let mut storage = Vec::with_capacity(std::mem::size_of::<usize>() + len);
    storage.extend_from_slice(&len.to_ne_bytes());
    storage.extend_from_slice(&data);
    let mut storage = storage.into_boxed_slice();
    let base = storage.as_mut_ptr();
    std::mem::forget(storage);

    // The caller only ever sees the data half; the length stays behind it.
    (unsafe { base.add(std::mem::size_of::<usize>()) }, len)
}

pub fn free<T>(_x: T) {
    drop(_x);
}

/// Frees a `Box<T>` that was handed to C, so `T` MUST be the concrete type the pointer was
/// allocated with. A pointer that C received as `c_void` has to be re-typed by its owner
/// (`Box::from_raw(ptr as *mut ConcreteType)`): freeing it here as `c_void` would both skip
/// `T`'s destructor and deallocate with the wrong layout.
pub fn ngenrs_free_ptr<T>(raw: *mut T) {
    if !raw.is_null() {
        unsafe { free(Box::from_raw(raw)) };
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_free_cstr(s: *mut c_char) {
    ffi_return! {
        if !s.is_null() {
            free(unsafe { CString::from_raw(s) });
        }
    }
}

/// Releases a buffer of [`rust_to_cbytes`].
///
/// The length is read back from the allocation rather than taken from the caller, which is what
/// makes a mismatched `len` impossible: an earlier version of this function rebuilt a
/// `Box<[u8]>` from the caller's number, so a number that was too large deallocated with the wrong
/// layout. A null pointer is a no-op, so an empty result needs no special case.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_free_bytes(buf: *mut u8) {
    ffi_return! {
        if buf.is_null() {
            return;
        }

        let head = unsafe { buf.sub(std::mem::size_of::<usize>()) };
        let mut len_bytes = [0u8; std::mem::size_of::<usize>()];
        unsafe { std::ptr::copy_nonoverlapping(head, len_bytes.as_mut_ptr(), len_bytes.len()) };
        let len = usize::from_ne_bytes(len_bytes);

        // The allocation was `size_of::<usize>() + len` bytes, and that is how it has to be rebuilt.
        let total = std::mem::size_of::<usize>() + len;
        unsafe {
            drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                head, total,
            )))
        };
    }
}

pub fn box_into_raw_new<T>(value: T) -> *mut T {
    Box::into_raw(Box::new(value))
}

/// # Safety
///
/// `keys` and `values` must each point to `len` readable `*const c_char` entries, and every
/// non-null entry must point to a NUL-terminated string that stays valid for the call.
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
        if let (Some(key), Some(value)) =
            (cstr_to_rust(keys_slice[i]), cstr_to_rust(values_slice[i]))
        {
            map.insert(key.to_string(), value.to_string());
        }
    }
    Some(map)
}

/// Converts a Rust `HashMap` into two parallel arrays of C strings, writing at most as many
/// entries as the caller's arrays can hold.
///
/// `written_out` receives the number of entries actually written, which is `map.len()` unless the
/// caller's arrays were smaller — that is the truncation, and it is not an error.
///
/// `Err` when a key or value holds a NUL, because a C string cannot carry one. Writing a null entry
/// instead, which is what this used to do, left the caller with an unreadable entry that looked like
/// an empty string.
///
/// # Safety
///
/// `keys_out` must point to `keys_cap` writable `*mut c_char` slots and `values_out` to
/// `values_cap` of them; `written_out` is null or points to a writable `usize`. Every entry
/// written before an error is owned by the caller, which releases it with `ngenrs_free_cstr`.
pub unsafe fn rust_map_to_c_arrays(
    map: &HashMap<String, String>,
    keys_out: *mut *mut c_char,
    keys_cap: usize,
    values_out: *mut *mut c_char,
    values_cap: usize,
    written_out: *mut usize,
) -> Result<usize, std::ffi::NulError> {
    let mut written = 0;
    let cap = map.len().min(keys_cap).min(values_cap);
    let usable = !keys_out.is_null() && !values_out.is_null();

    // Written before anything can fail, so the caller never reads a stale count on the error path.
    if !written_out.is_null() {
        unsafe { *written_out = 0 };
    }

    if usable {
        for (key, value) in map.iter().take(cap) {
            // Both conversions happen before anything is written, so a failure here cannot leave a
            // half-filled entry behind.
            let key = rust_to_cstr(key.clone())?;
            let value = rust_to_cstr(value.clone())?;
            unsafe {
                keys_out.add(written).write(key);
                values_out.add(written).write(value);
            }
            written += 1;
        }
    }

    if !written_out.is_null() {
        unsafe { *written_out = written };
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The arrays a caller can offer are what bounds the write. Before the capacity arguments
    /// existed, a map with more entries than the array held was written straight past the end:
    /// ten entries into two slots corrupted the neighbouring locals and the count itself.
    #[test]
    fn more_entries_than_slots_are_truncated_rather_than_written_past_the_end() {
        let map: HashMap<String, String> = (0..10)
            .map(|i| (format!("k{i}"), format!("v{i}")))
            .collect();

        let mut keys = [std::ptr::null_mut::<c_char>(); 2];
        let mut values = [std::ptr::null_mut::<c_char>(); 2];
        let mut count: usize = 0;

        let written = unsafe {
            rust_map_to_c_arrays(
                &map,
                keys.as_mut_ptr(),
                keys.len(),
                values.as_mut_ptr(),
                values.len(),
                &mut count,
            )
        }
        .expect("these keys and values are text");

        // The arrays hold two, so two are written and the caller can see that the rest was lost by
        // comparing against `map.len()`.
        assert_eq!(written, 2);
        assert_eq!(count, 2);
        assert!(
            written < map.len(),
            "the truncation is visible to the caller"
        );
        assert!(!keys[0].is_null(), "the first slot was written");

        for entry in keys.into_iter().chain(values) {
            ngenrs_free_cstr(entry);
        }
    }

    /// A capacity of zero is accepted rather than treated as an error: it is how a caller asks how
    /// many entries there are without taking any.
    #[test]
    fn a_zero_capacity_reports_zero_and_writes_nothing() {
        let map: HashMap<String, String> =
            [("k".to_string(), "v".to_string())].into_iter().collect();
        let mut count: usize = 7;

        let written = unsafe {
            rust_map_to_c_arrays(
                &map,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                0,
                &mut count,
            )
        }
        .expect("a zero capacity is not an error");

        assert_eq!(written, 0);
        assert_eq!(count, 0);
    }

    /// A value a C string cannot carry is reported rather than written as a null entry, which is
    /// what used to happen: the caller could not tell it from an empty string.
    #[test]
    fn a_value_with_a_nul_is_an_error_rather_than_a_null_entry() {
        let map: HashMap<String, String> = [("k\0ey".to_string(), "value".to_string())]
            .into_iter()
            .collect();
        let mut keys = [std::ptr::null_mut::<c_char>(); 1];
        let mut values = [std::ptr::null_mut::<c_char>(); 1];
        let mut count = usize::MAX;

        let result = unsafe {
            rust_map_to_c_arrays(
                &map,
                keys.as_mut_ptr(),
                keys.len(),
                values.as_mut_ptr(),
                values.len(),
                &mut count,
            )
        };

        assert!(result.is_err(), "the key cannot become a C string");
        assert_eq!(count, 0, "nothing was written");
        assert!(keys[0].is_null());
        assert!(values[0].is_null());
    }
}
