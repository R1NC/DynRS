use crate::c::util::{box_into_raw_new, cstr_to_rust, ngenrs_free_cstr, rust_to_cstr};
use crate::core::kv::KV;
use std::os::raw::{c_char, c_void};

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_kv_open(path: *const c_char) -> *mut c_void {
    let path_str = match cstr_to_rust(path) {
        Some(s) => s,
        None => return std::ptr::null_mut(),
    };

    match KV::open(path_str) {
        Ok(store) => box_into_raw_new(store) as *mut c_void,
        Err(_) => std::ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_kv_write_int(store: *mut c_void, key: *const c_char, value: i64) -> bool {
    if store.is_null() {
        return false;
    }
    let key_str = match cstr_to_rust(key) {
        Some(s) => s,
        None => return false,
    };
    unsafe {
        let kv_ref = &mut *(store as *mut KV);
        kv_ref.write_int(key_str, value).is_ok()
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_kv_read_int(store: *mut c_void, key: *const c_char) -> i64 {
    if store.is_null() {
        return 0;
    }
    let key_str = match cstr_to_rust(key) {
        Some(s) => s,
        None => return 0,
    };
    unsafe {
        let kv_ref = &mut *(store as *mut KV);
        match kv_ref.read_int(key_str) {
            Ok(value) => value.unwrap_or(0),
            Err(_) => 0,
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_kv_write_float(
    store: *mut c_void,
    key: *const c_char,
    value: f64,
) -> bool {
    if store.is_null() {
        return false;
    }
    let key_str = match cstr_to_rust(key) {
        Some(s) => s,
        None => return false,
    };
    unsafe {
        let kv_ref = &mut *(store as *mut KV);
        kv_ref.write_float(key_str, value).is_ok()
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_kv_read_float(store: *mut c_void, key: *const c_char) -> f64 {
    if store.is_null() {
        return 0.0;
    }
    let key_str = match cstr_to_rust(key) {
        Some(s) => s,
        None => return 0.0,
    };
    unsafe {
        let kv_ref = &mut *(store as *mut KV);
        match kv_ref.read_float(key_str) {
            Ok(value) => value.unwrap_or(0.0),
            Err(_) => 0.0,
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_kv_write_string(
    store: *mut c_void,
    key: *const c_char,
    value: *const c_char,
) -> bool {
    if store.is_null() {
        return false;
    }
    let key_str = match cstr_to_rust(key) {
        Some(s) => s,
        None => return false,
    };
    let value_str = cstr_to_rust(value).unwrap_or_default();
    unsafe {
        let kv_ref = &mut *(store as *mut KV);
        kv_ref.write_string(key_str, value_str).is_ok()
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_kv_read_string(store: *mut c_void, key: *const c_char) -> *mut c_char {
    if store.is_null() {
        return std::ptr::null_mut();
    }
    let key_str = match cstr_to_rust(key) {
        Some(s) => s,
        None => return std::ptr::null_mut(),
    };
    unsafe {
        let kv_ref = &mut *(store as *mut KV);
        match kv_ref.read_string(key_str) {
            Ok(value) => match value {
                Some(s) => rust_to_cstr(s),
                None => std::ptr::null_mut(),
            },
            Err(_) => std::ptr::null_mut(),
        }
    }
}

/// Mirrors `dynxx_kv_contains`: false for a null handle, a null key or an empty key.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_kv_contains(store: *mut c_void, key: *const c_char) -> bool {
    if store.is_null() {
        return false;
    }
    let Some(key_str) = cstr_to_rust(key) else {
        return false;
    };
    let kv_ref = unsafe { &*(store as *const KV) };
    kv_ref.contains(key_str).unwrap_or(false)
}

/// Mirrors `dynxx_kv_remove`: false when there was nothing to remove.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_kv_remove(store: *mut c_void, key: *const c_char) -> bool {
    if store.is_null() {
        return false;
    }
    let Some(key_str) = cstr_to_rust(key) else {
        return false;
    };
    let kv_ref = unsafe { &*(store as *const KV) };
    kv_ref.remove(key_str).unwrap_or(false)
}

/// Mirrors `dynxx_kv_all_keys`: an array of key strings, with its length in `count_out`.
/// Returns null and a zero count when the store is unknown, the query fails or the store is empty.
/// Release the result with `ngenrs_kv_free_keys`.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_kv_all_keys(
    store: *mut c_void,
    count_out: *mut usize,
) -> *mut *mut c_char {
    if !count_out.is_null() {
        unsafe { *count_out = 0 };
    }
    if store.is_null() || count_out.is_null() {
        return std::ptr::null_mut();
    }

    let kv_ref = unsafe { &*(store as *const KV) };
    let Ok(keys) = kv_ref.all_keys() else {
        return std::ptr::null_mut();
    };
    if keys.is_empty() {
        return std::ptr::null_mut();
    }

    let mut array: Vec<*mut c_char> = keys.into_iter().map(rust_to_cstr).collect();
    // The caller rebuilds a `Vec` from the pointer, so its length has to match its capacity.
    array.shrink_to_fit();
    let len = array.len();
    let ptr = array.as_mut_ptr();
    std::mem::forget(array);

    unsafe { *count_out = len };
    ptr
}

/// Releases an array returned by `ngenrs_kv_all_keys` together with its strings.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_kv_free_keys(keys: *mut *mut c_char, count: usize) {
    if keys.is_null() {
        return;
    }
    let entries = unsafe { Vec::from_raw_parts(keys, count, count) };
    for entry in entries {
        ngenrs_free_cstr(entry);
    }
}

/// Mirrors `dynxx_kv_clear`: a null handle is ignored.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_kv_clear(store: *mut c_void) {
    if store.is_null() {
        return;
    }
    let kv_ref = unsafe { &*(store as *const KV) };
    let _ = kv_ref.clear();
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_kv_close(store: *mut c_void) {
    if !store.is_null() {
        unsafe { drop(Box::from_raw(store as *mut KV)) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::{CStr, CString};
    use std::path::PathBuf;

    /// A redb file under the system temp directory that removes itself, and any leftover of
    /// an earlier run, on drop. A test declares it before the store so the store is closed
    /// first, which Windows needs before the file can be removed again.
    struct TempDbFile {
        path: PathBuf,
    }

    impl TempDbFile {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("dynrs_kv_c_{name}_{}.redb", std::process::id()));
            let _ = std::fs::remove_file(&path);
            Self { path }
        }

        fn to_cstring(&self) -> CString {
            CString::new(self.path.to_str().expect("the temp path is valid UTF-8")).unwrap()
        }
    }

    impl Drop for TempDbFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    /// Copies the array `ngenrs_kv_all_keys` returned into owned strings.
    fn read_keys(keys: *mut *mut c_char, count: usize) -> Vec<String> {
        assert!(!keys.is_null(), "a non-empty store returns an array");
        let entries = unsafe { std::slice::from_raw_parts(keys, count) };
        entries
            .iter()
            .map(|entry| {
                unsafe { CStr::from_ptr(*entry) }
                    .to_string_lossy()
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn null_arguments_are_rejected() {
        assert!(ngenrs_kv_open(std::ptr::null()).is_null());
        assert!(!ngenrs_kv_write_int(
            std::ptr::null_mut(),
            std::ptr::null(),
            1
        ));
        assert_eq!(
            ngenrs_kv_read_int(std::ptr::null_mut(), std::ptr::null()),
            0
        );
        assert!(!ngenrs_kv_write_float(
            std::ptr::null_mut(),
            std::ptr::null(),
            1.0
        ));
        assert_eq!(
            ngenrs_kv_read_float(std::ptr::null_mut(), std::ptr::null()),
            0.0
        );
        assert!(!ngenrs_kv_write_string(
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null()
        ));
        assert!(ngenrs_kv_read_string(std::ptr::null_mut(), std::ptr::null()).is_null());
        assert!(!ngenrs_kv_contains(std::ptr::null_mut(), std::ptr::null()));
        assert!(!ngenrs_kv_remove(std::ptr::null_mut(), std::ptr::null()));

        let mut count = usize::MAX;
        assert!(ngenrs_kv_all_keys(std::ptr::null_mut(), &mut count).is_null());
        assert_eq!(count, 0, "a failed listing reports an empty array");
        ngenrs_kv_free_keys(std::ptr::null_mut(), 0);

        // Closing or clearing a null handle is a no-op.
        ngenrs_kv_close(std::ptr::null_mut());
        ngenrs_kv_clear(std::ptr::null_mut());
    }

    #[test]
    fn values_round_trip_through_the_c_abi() {
        let file = TempDbFile::new("round_trip");
        let path = file.to_cstring();
        let store = ngenrs_kv_open(path.as_ptr());
        assert!(!store.is_null(), "store opens");

        let key = CString::new("k").unwrap();
        let value = CString::new("v").unwrap();
        assert!(ngenrs_kv_write_int(store, key.as_ptr(), -3));
        assert!(ngenrs_kv_write_float(store, key.as_ptr(), 0.25));
        assert!(ngenrs_kv_write_string(store, key.as_ptr(), value.as_ptr()));

        assert_eq!(ngenrs_kv_read_int(store, key.as_ptr()), -3);
        assert_eq!(ngenrs_kv_read_float(store, key.as_ptr()), 0.25);

        let read = ngenrs_kv_read_string(store, key.as_ptr());
        assert!(!read.is_null());
        assert_eq!(unsafe { CStr::from_ptr(read) }.to_str().unwrap(), "v");
        ngenrs_free_cstr(read);

        // A missing key reads as the C fallback of each type.
        let missing = CString::new("missing").unwrap();
        assert_eq!(ngenrs_kv_read_int(store, missing.as_ptr()), 0);
        assert_eq!(ngenrs_kv_read_float(store, missing.as_ptr()), 0.0);
        assert!(ngenrs_kv_read_string(store, missing.as_ptr()).is_null());

        ngenrs_kv_close(store);
    }

    #[test]
    fn values_survive_close_and_a_reopen() {
        let file = TempDbFile::new("reopen");
        let path = file.to_cstring();

        let store = ngenrs_kv_open(path.as_ptr());
        assert!(!store.is_null(), "store opens");
        let key = CString::new("k").unwrap();
        let value = CString::new("kept").unwrap();
        assert!(ngenrs_kv_write_string(store, key.as_ptr(), value.as_ptr()));
        ngenrs_kv_close(store);

        let store = ngenrs_kv_open(path.as_ptr());
        assert!(!store.is_null(), "store reopens");
        let read = ngenrs_kv_read_string(store, key.as_ptr());
        assert!(!read.is_null());
        assert_eq!(unsafe { CStr::from_ptr(read) }.to_str().unwrap(), "kept");
        ngenrs_free_cstr(read);
        ngenrs_kv_close(store);
    }

    #[test]
    fn open_rejects_an_empty_path() {
        let path = CString::new("").unwrap();
        assert!(ngenrs_kv_open(path.as_ptr()).is_null());
    }

    #[test]
    fn contains_remove_all_keys_and_clear_through_the_c_abi() {
        let file = TempDbFile::new("collections");
        let path = file.to_cstring();
        let store = ngenrs_kv_open(path.as_ptr());
        assert!(!store.is_null(), "store opens");

        let a = CString::new("a").unwrap();
        let b = CString::new("b").unwrap();
        let missing = CString::new("missing").unwrap();
        let value = CString::new("value").unwrap();
        assert!(ngenrs_kv_write_string(store, a.as_ptr(), value.as_ptr()));
        assert!(ngenrs_kv_write_int(store, b.as_ptr(), 2));

        assert!(ngenrs_kv_contains(store, a.as_ptr()));
        assert!(ngenrs_kv_contains(store, b.as_ptr()));
        assert!(!ngenrs_kv_contains(store, missing.as_ptr()));

        // All keys come back sorted, with the count in the out parameter.
        let mut count = 0;
        let keys = ngenrs_kv_all_keys(store, &mut count);
        assert_eq!(count, 2);
        assert_eq!(read_keys(keys, count), ["a", "b"]);
        ngenrs_kv_free_keys(keys, count);

        assert!(ngenrs_kv_remove(store, a.as_ptr()));
        assert!(!ngenrs_kv_contains(store, a.as_ptr()));
        // Removing it again reports that there was nothing left to remove.
        assert!(!ngenrs_kv_remove(store, a.as_ptr()));

        ngenrs_kv_clear(store);
        assert!(!ngenrs_kv_contains(store, b.as_ptr()));
        let mut count = usize::MAX;
        assert!(
            ngenrs_kv_all_keys(store, &mut count).is_null(),
            "a cleared store lists nothing"
        );
        assert_eq!(count, 0);

        ngenrs_kv_close(store);
    }

    #[test]
    fn an_empty_key_is_refused_through_the_c_abi() {
        let file = TempDbFile::new("empty_key");
        let path = file.to_cstring();
        let store = ngenrs_kv_open(path.as_ptr());
        assert!(!store.is_null(), "store opens");

        let empty = CString::new("").unwrap();
        let value = CString::new("v").unwrap();
        assert!(!ngenrs_kv_write_int(store, empty.as_ptr(), 1));
        assert!(!ngenrs_kv_write_float(store, empty.as_ptr(), 1.5));
        assert!(!ngenrs_kv_write_string(
            store,
            empty.as_ptr(),
            value.as_ptr()
        ));
        assert_eq!(ngenrs_kv_read_int(store, empty.as_ptr()), 0);
        assert!(ngenrs_kv_read_string(store, empty.as_ptr()).is_null());
        assert!(!ngenrs_kv_contains(store, empty.as_ptr()));
        assert!(!ngenrs_kv_remove(store, empty.as_ptr()));

        ngenrs_kv_close(store);
    }
}
