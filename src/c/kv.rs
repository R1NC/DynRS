use crate::c::util::{box_into_raw_new, cstr_to_rust, rust_to_cstr};
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

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_kv_close(store: *mut c_void) {
    if !store.is_null() {
        unsafe { drop(Box::from_raw(store as *mut KV)) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::c::util::ngenrs_free_cstr;
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

        // Closing a null handle is a no-op.
        ngenrs_kv_close(std::ptr::null_mut());
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
}
