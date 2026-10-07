//! The key-value store entry points.
//!
//! Every one of them reports a [`DynrsStatus`] and writes its result through out-parameters. The
//! distinction the status carries is the point: a rejected argument, a key that is not there, and a
//! store that failed used to be one `false`, one `0` or one null pointer.
//!
//! `DynrsStatus::Empty` means "no value under this key" and `DynrsStatus::Failed` means "the store
//! could not answer". Values are handed over as bytes with a length, because a stored value or a
//! key can contain a NUL that a C string could not carry.

use crate::DynrsStatus;
use crate::c::util::{box_into_raw_new, bytes_to_c, cstr_to_rust, ngenrs_free_bytes};
use crate::core::kv::{KV, KvError};
use std::os::raw::{c_char, c_void};

/// Maps a store failure onto the status the C caller sees.
fn store_status(error: KvError) -> DynrsStatus {
    match error {
        // An empty key is a rule of the API, not a failure of the store.
        KvError::EmptyKey => DynrsStatus::InvalidArgument,
        KvError::Store(_) => DynrsStatus::Failed,
    }
}

/// Borrows a store handle, or reports it as invalid.
fn store_ref<'a>(store: *const c_void) -> Result<&'a KV, DynrsStatus> {
    unsafe { (store as *const KV).as_ref() }.ok_or(DynrsStatus::InvalidHandle)
}

/// Reads a required key argument.
fn required_key(key: *const c_char) -> Result<String, DynrsStatus> {
    if key.is_null() {
        return Err(DynrsStatus::InvalidArgument);
    }
    cstr_to_rust(key)
        .map(str::to_string)
        .ok_or(DynrsStatus::InvalidArgument)
}

/// Runs a store-and-key operation and writes its result through an out-parameter.
///
/// The nine operations that share this shape used to repeat the same three steps — check the out
/// pointer, borrow the store, read the key — so the rules for a null handle and an empty key lived
/// in nine places. The operation returns `Result<Option<T>, KvError>`: `Ok(None)` means the store
/// has no value to report, which becomes [`DynrsStatus::Empty`].
macro_rules! kv_operation {
    ($store:expr, $key:expr, $out:expr, |$kv:ident, $key_str:ident| $body:expr) => {{
        if $out.is_null() {
            return DynrsStatus::InvalidArgument;
        }
        let $kv = match store_ref($store) {
            Ok(store) => store,
            Err(status) => return status,
        };
        let $key_str = match required_key($key) {
            Ok(key) => key,
            Err(status) => return status,
        };
        match $body {
            Ok(Some(value)) => {
                unsafe { *$out = value };
                DynrsStatus::Ok
            }
            Ok(None) => DynrsStatus::Empty,
            Err(error) => store_status(error),
        }
    }};
}

/// The same, for an operation with nothing to report: it either worked or it did not.
macro_rules! kv_side_effect {
    ($store:expr, $key:expr, |$kv:ident, $key_str:ident| $body:expr) => {{
        let $kv = match store_ref($store) {
            Ok(store) => store,
            Err(status) => return status,
        };
        let $key_str = match required_key($key) {
            Ok(key) => key,
            Err(status) => return status,
        };
        match $body {
            Ok(()) => DynrsStatus::Ok,
            Err(error) => store_status(error),
        }
    }};
}

/// The same, for an operation that reports whether it changed anything: a real "nothing was there"
/// becomes [`DynrsStatus::Empty`], not a failure.
macro_rules! kv_change {
    ($store:expr, $key:expr, |$kv:ident, $key_str:ident| $body:expr) => {{
        let $kv = match store_ref($store) {
            Ok(store) => store,
            Err(status) => return status,
        };
        let $key_str = match required_key($key) {
            Ok(key) => key,
            Err(status) => return status,
        };
        match $body {
            Ok(true) => DynrsStatus::Ok,
            Ok(false) => DynrsStatus::Empty,
            Err(error) => store_status(error),
        }
    }};
}

/// The same, for an operation that answers yes or no. `Ok(false)` is the store answering "no",
/// which is a different thing from a lookup that failed.
macro_rules! kv_predicate {
    ($store:expr, $key:expr, $out:expr, |$kv:ident, $key_str:ident| $body:expr) => {{
        if $out.is_null() {
            return DynrsStatus::InvalidArgument;
        }
        let $kv = match store_ref($store) {
            Ok(store) => store,
            Err(status) => return status,
        };
        let $key_str = match required_key($key) {
            Ok(key) => key,
            Err(status) => return status,
        };
        match $body {
            Ok(answer) => {
                unsafe { *$out = answer };
                DynrsStatus::Ok
            }
            Err(error) => store_status(error),
        }
    }};
}

/// Opens a store, handing the handle back through `out`.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_kv_open(path: *const c_char, out: *mut *mut c_void) -> DynrsStatus {
    ffi_return! {
        if !out.is_null() {
            unsafe { *out = std::ptr::null_mut() };
        }
        if path.is_null() {
            return DynrsStatus::InvalidArgument;
        }
        let Some(path_str) = cstr_to_rust(path) else {
            return DynrsStatus::InvalidArgument;
        };
        if out.is_null() {
            return DynrsStatus::InvalidArgument;
        }

        match KV::open(path_str) {
            Ok(store) => {
                unsafe { *out = box_into_raw_new(store) as *mut c_void };
                DynrsStatus::Ok
            }
            Err(_) => DynrsStatus::Failed,
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_kv_write_int(
    store: *mut c_void,
    key: *const c_char,
    value: i64,
) -> DynrsStatus {
    ffi_return! {
        kv_side_effect!(store, key, |kv, key| kv.write_int(&key, value))
    }
}

/// Reads an integer.
///
/// [`DynrsStatus::Empty`] means the key holds no integer, which is a different answer from a store
/// that failed and from an out-of-range zero that a caller could mistake for data.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_kv_read_int(
    store: *mut c_void,
    key: *const c_char,
    out: *mut i64,
) -> DynrsStatus {
    ffi_return! {
        kv_operation!(store, key, out, |kv, key| kv.read_int(&key))
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_kv_write_float(
    store: *mut c_void,
    key: *const c_char,
    value: f64,
) -> DynrsStatus {
    ffi_return! {
        kv_side_effect!(store, key, |kv, key| kv.write_float(&key, value))
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_kv_read_float(
    store: *mut c_void,
    key: *const c_char,
    out: *mut f64,
) -> DynrsStatus {
    ffi_return! {
        kv_operation!(store, key, out, |kv, key| kv.read_float(&key))
    }
}

/// Writes a string value. A null or unreadable value pointer is refused rather than stored as the
/// empty string, which is what `unwrap_or_default` used to do while reporting success.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_kv_write_string(
    store: *mut c_void,
    key: *const c_char,
    value: *const c_char,
) -> DynrsStatus {
    ffi_return! {
        let store = match store_ref(store) {
            Ok(store) => store,
            Err(status) => return status,
        };
        let key = match required_key(key) {
            Ok(key) => key,
            Err(status) => return status,
        };
        let Some(value) = cstr_to_rust(value) else {
            return DynrsStatus::InvalidArgument;
        };
        match store.write_string(&key, value) {
            Ok(()) => DynrsStatus::Ok,
            Err(error) => store_status(error),
        }
    }
}

/// Reads a string value as bytes, writing the buffer through `out` and its length through `len_out`.
///
/// Release a non-null buffer with `ngenrs_free_bytes(ptr)`.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_kv_read_string(
    store: *mut c_void,
    key: *const c_char,
    out: *mut *mut u8,
    len_out: *mut usize,
) -> DynrsStatus {
    ffi_return! {
        if !len_out.is_null() {
            unsafe { *len_out = 0 };
        }
        kv_operation!(store, key, out, |kv, key| kv
            .read_string(&key)
            .map(|value| value.map(|text| bytes_to_c(text.into_bytes(), len_out))))
    }
}

/// Reports whether the key holds anything, through `out`.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_kv_contains(
    store: *mut c_void,
    key: *const c_char,
    out: *mut bool,
) -> DynrsStatus {
    ffi_return! {
        kv_predicate!(store, key, out, |kv, key| kv.contains(&key))
    }
}

/// Drops the key from every typed table.
///
/// [`DynrsStatus::Empty`] means there was nothing to remove, which used to be the same `false` a
/// failed store produced.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_kv_remove(store: *mut c_void, key: *const c_char) -> DynrsStatus {
    ffi_return! {
        kv_change!(store, key, |kv, key| kv.remove(&key))
    }
}

/// The key list `ngenrs_kv_all_keys` hands out.
///
/// It is deliberately opaque: the caller passes the pointer back to [`ngenrs_kv_key_count`],
/// [`ngenrs_kv_key_at`] and [`ngenrs_kv_free_keys`] and never sees the array's length or capacity.
///
/// The keys are held as byte buffers rather than C strings. A `redb` key is a Rust `&str` and may
/// contain a NUL; converting it to a C string produced a null entry in the array, so the key was
/// silently lost and [`ngenrs_kv_key_at`] handed the null back as if it were a key.
struct KvKeys {
    keys: Vec<(*mut u8, usize)>,
}

impl Drop for KvKeys {
    fn drop(&mut self) {
        for (ptr, _) in &self.keys {
            ngenrs_free_bytes(*ptr);
        }
    }
}

/// Lists the keys of the store, handing the handle back through `out`.
///
/// A store with no keys is [`DynrsStatus::Ok`] with a handle that reports a count of zero: an empty
/// list is a value, and it used to be the same null a failed query produced. Read the result with
/// [`ngenrs_kv_key_count`] and [`ngenrs_kv_key_at`], and release it with [`ngenrs_kv_free_keys`].
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_kv_all_keys(store: *mut c_void, out: *mut *mut c_void) -> DynrsStatus {
    ffi_return! {
        if !out.is_null() {
            unsafe { *out = std::ptr::null_mut() };
        }
        let store = match store_ref(store) {
            Ok(store) => store,
            Err(status) => return status,
        };
        if out.is_null() {
            return DynrsStatus::InvalidArgument;
        }

        match store.all_keys() {
            Ok(keys) => {
                let keys = KvKeys {
                    keys: keys
                        .into_iter()
                        .map(|key| bytes_to_c(key.into_bytes(), std::ptr::null_mut()))
                        .map(|ptr| {
                            // The length is needed to read a key back, so it is measured here from
                            // the allocation the conversion just made.
                            let len = key_length(ptr);
                            (ptr, len)
                        })
                        .collect(),
                };
                unsafe { *out = box_into_raw_new(keys) as *mut c_void };
                DynrsStatus::Ok
            }
            Err(error) => store_status(error),
        }
    }
}

/// The length of a buffer `bytes_to_c` produced, read back from its length prefix.
fn key_length(ptr: *mut u8) -> usize {
    if ptr.is_null() {
        return 0;
    }
    let head = unsafe { ptr.sub(std::mem::size_of::<usize>()) };
    let mut bytes = [0u8; std::mem::size_of::<usize>()];
    unsafe { std::ptr::copy_nonoverlapping(head, bytes.as_mut_ptr(), bytes.len()) };
    usize::from_ne_bytes(bytes)
}

/// Writes how many keys the handle holds through `out`. A store with no keys reports a zero count,
/// not a failure.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_kv_key_count(keys: *const c_void, out: *mut usize) -> DynrsStatus {
    ffi_return! {
        if out.is_null() {
            return DynrsStatus::InvalidArgument;
        }
        unsafe { *out = 0 };
        let Some(keys) = (unsafe { (keys as *const KvKeys).as_ref() }) else {
            return DynrsStatus::InvalidHandle;
        };
        unsafe { *out = keys.keys.len() };
        DynrsStatus::Ok
    }
}

/// Hands out the key at `index` as bytes: the buffer through `out`, its length through `len_out`.
///
/// The bytes belong to the handle and are released with it; do not free them on their own.
/// [`DynrsStatus::Empty`] means the index is past the end.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_kv_key_at(
    keys: *const c_void,
    index: usize,
    out: *mut *const u8,
    len_out: *mut usize,
) -> DynrsStatus {
    ffi_return! {
        if !out.is_null() {
            unsafe { *out = std::ptr::null() };
        }
        if !len_out.is_null() {
            unsafe { *len_out = 0 };
        }
        let Some(keys) = (unsafe { (keys as *const KvKeys).as_ref() }) else {
            return DynrsStatus::InvalidHandle;
        };
        if out.is_null() {
            return DynrsStatus::InvalidArgument;
        }
        match keys.keys.get(index) {
            Some((ptr, len)) => {
                unsafe {
                    *out = *ptr as *const u8;
                    if !len_out.is_null() {
                        *len_out = *len;
                    }
                }
                DynrsStatus::Ok
            }
            None => DynrsStatus::Empty,
        }
    }
}

/// Releases a key list returned by `ngenrs_kv_all_keys` together with its keys. A null handle is a
/// no-op.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_kv_free_keys(keys: *mut c_void) {
    ffi_return! {
        if keys.is_null() {
            return;
        }
        // The lengths are the ones this handle was allocated with, so there is nothing for the
        // caller to get wrong.
        drop(unsafe { Box::from_raw(keys as *mut KvKeys) });
    }
}

/// Removes every key.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_kv_clear(store: *mut c_void) -> DynrsStatus {
    ffi_return! {
        let store = match store_ref(store) {
            Ok(store) => store,
            Err(status) => return status,
        };
        // The error is reported rather than discarded: a clear that did not happen is worth knowing
        // about, and `let _ =` was no channel at all.
        match store.clear() {
            Ok(()) => DynrsStatus::Ok,
            Err(error) => store_status(error),
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_kv_close(store: *mut c_void) {
    ffi_return! {
        if !store.is_null() {
            unsafe { drop(Box::from_raw(store as *mut KV)) };
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;
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

    /// Opens a store through the C ABI, asserting that it worked.
    fn open_store(path: &CString) -> *mut c_void {
        let mut store = std::ptr::null_mut();
        assert_eq!(
            ngenrs_kv_open(path.as_ptr(), &mut store),
            DynrsStatus::Ok,
            "the store opens"
        );
        store
    }

    /// Writes an integer, asserting the status.
    fn write_int(store: *mut c_void, key: &CString, value: i64) -> DynrsStatus {
        ngenrs_kv_write_int(store, key.as_ptr(), value)
    }

    /// Reads an integer through the out-parameter form.
    fn read_int(store: *mut c_void, key: &CString) -> Result<i64, DynrsStatus> {
        let mut value = 0i64;
        match ngenrs_kv_read_int(store, key.as_ptr(), &mut value) {
            DynrsStatus::Ok => Ok(value),
            other => Err(other),
        }
    }

    /// Reads a real through the out-parameter form.
    fn read_float(store: *mut c_void, key: &CString) -> Result<f64, DynrsStatus> {
        let mut value = 0.0f64;
        match ngenrs_kv_read_float(store, key.as_ptr(), &mut value) {
            DynrsStatus::Ok => Ok(value),
            other => Err(other),
        }
    }

    /// Reports whether the key is there. `Ok(false)` is the store answering "no", which is a
    /// different thing from a lookup that failed.
    fn contains(store: *mut c_void, key: &CString) -> Result<bool, DynrsStatus> {
        let mut present = false;
        match ngenrs_kv_contains(store, key.as_ptr(), &mut present) {
            DynrsStatus::Ok => Ok(present),
            other => Err(other),
        }
    }

    /// Lists the keys of a store, asserting the listing worked.
    fn list_keys(store: *mut c_void) -> Vec<String> {
        let mut keys = std::ptr::null_mut();
        assert_eq!(
            ngenrs_kv_all_keys(store, &mut keys),
            DynrsStatus::Ok,
            "the listing works"
        );
        assert!(!keys.is_null(), "a successful listing hands back a handle");
        let read = read_keys(keys);
        ngenrs_kv_free_keys(keys);
        read
    }

    /// Copies the keys `ngenrs_kv_all_keys` returned into owned strings, through the accessors the
    /// opaque handle offers.
    fn read_keys(keys: *mut c_void) -> Vec<String> {
        let mut count = 0usize;
        assert_eq!(
            ngenrs_kv_key_count(keys, &mut count),
            DynrsStatus::Ok,
            "the count is readable"
        );
        (0..count)
            .map(|index| {
                let mut ptr = std::ptr::null();
                let mut len = 0usize;
                assert_eq!(
                    ngenrs_kv_key_at(keys, index, &mut ptr, &mut len),
                    DynrsStatus::Ok,
                    "every index below the count has a key"
                );
                let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
                String::from_utf8(bytes.to_vec()).expect("the test keys are text")
            })
            .collect()
    }

    /// Copies a string value out through the length-bearing reader and releases it, the way a C
    /// caller does. `None` when the store holds no string under that key; an error when the call
    /// itself was refused.
    fn read_value(store: *mut c_void, key: &CString) -> Result<Option<String>, DynrsStatus> {
        read_value_raw(store, key.as_ptr())
    }

    fn read_value_raw(
        store: *mut c_void,
        key: *const c_char,
    ) -> Result<Option<String>, DynrsStatus> {
        let mut ptr = std::ptr::null_mut();
        let mut len = 0usize;
        match ngenrs_kv_read_string(store, key, &mut ptr, &mut len) {
            DynrsStatus::Ok => {
                let bytes = unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec();
                ngenrs_free_bytes(ptr);
                Ok(Some(
                    String::from_utf8(bytes).expect("the test data is text"),
                ))
            }
            DynrsStatus::Empty => Ok(None),
            other => Err(other),
        }
    }

    /// The text of a key list entry, for the tests that compare against literals.
    fn read_keys_text(store: *mut c_void) -> Vec<String> {
        list_keys(store)
    }

    #[test]
    fn null_arguments_are_rejected() {
        let mut store = std::ptr::null_mut();
        assert_eq!(
            ngenrs_kv_open(std::ptr::null(), &mut store),
            DynrsStatus::InvalidArgument
        );
        assert!(store.is_null());

        assert_eq!(
            ngenrs_kv_write_int(std::ptr::null_mut(), std::ptr::null(), 1),
            DynrsStatus::InvalidHandle
        );
        let mut int = 0i64;
        assert_eq!(
            ngenrs_kv_read_int(std::ptr::null_mut(), std::ptr::null(), &mut int),
            DynrsStatus::InvalidHandle
        );
        assert_eq!(
            ngenrs_kv_write_float(std::ptr::null_mut(), std::ptr::null(), 1.0),
            DynrsStatus::InvalidHandle
        );
        let mut real = 0.0f64;
        assert_eq!(
            ngenrs_kv_read_float(std::ptr::null_mut(), std::ptr::null(), &mut real),
            DynrsStatus::InvalidHandle
        );
        assert_eq!(
            ngenrs_kv_write_string(std::ptr::null_mut(), std::ptr::null(), std::ptr::null()),
            DynrsStatus::InvalidHandle
        );
        let mut ptr = std::ptr::null_mut();
        let mut len = usize::MAX;
        assert_eq!(
            ngenrs_kv_read_string(std::ptr::null_mut(), std::ptr::null(), &mut ptr, &mut len),
            DynrsStatus::InvalidHandle
        );
        assert!(ptr.is_null());
        assert_eq!(len, 0, "a refused call reports a zero length");
        let mut present = false;
        assert_eq!(
            ngenrs_kv_contains(std::ptr::null_mut(), std::ptr::null(), &mut present),
            DynrsStatus::InvalidHandle
        );
        assert_eq!(
            ngenrs_kv_remove(std::ptr::null_mut(), std::ptr::null()),
            DynrsStatus::InvalidHandle
        );

        // A null store yields no key list, and the accessors and the release all accept null.
        let mut keys = std::ptr::null_mut();
        assert_eq!(
            ngenrs_kv_all_keys(std::ptr::null_mut(), &mut keys),
            DynrsStatus::InvalidHandle
        );
        assert!(keys.is_null());
        let mut count = usize::MAX;
        assert_eq!(
            ngenrs_kv_key_count(std::ptr::null(), &mut count),
            DynrsStatus::InvalidHandle
        );
        assert_eq!(count, 0);
        let mut key_ptr = std::ptr::null();
        assert_eq!(
            ngenrs_kv_key_at(std::ptr::null(), 0, &mut key_ptr, &mut len),
            DynrsStatus::InvalidHandle
        );
        ngenrs_kv_free_keys(std::ptr::null_mut());

        // Closing a null handle is a no-op; clearing it cannot even find a store.
        ngenrs_kv_close(std::ptr::null_mut());
        assert_eq!(
            ngenrs_kv_clear(std::ptr::null_mut()),
            DynrsStatus::InvalidHandle
        );
    }

    #[test]
    fn values_round_trip_through_the_c_abi() {
        let file = TempDbFile::new("round_trip");
        let path = file.to_cstring();
        let store = open_store(&path);

        let key = CString::new("k").unwrap();
        let value = CString::new("v").unwrap();
        assert_eq!(write_int(store, &key, -3), DynrsStatus::Ok);
        assert_eq!(
            ngenrs_kv_write_float(store, key.as_ptr(), 0.25),
            DynrsStatus::Ok
        );
        assert_eq!(
            ngenrs_kv_write_string(store, key.as_ptr(), value.as_ptr()),
            DynrsStatus::Ok
        );

        assert_eq!(read_int(store, &key), Ok(-3));
        assert_eq!(read_float(store, &key), Ok(0.25));
        assert_eq!(read_value(store, &key), Ok(Some("v".to_string())));

        // A key that is not there is `Empty`, which is a different answer from a store failure and
        // from the zero a caller used to receive.
        let missing = CString::new("missing").unwrap();
        assert_eq!(read_int(store, &missing), Err(DynrsStatus::Empty));
        assert_eq!(read_float(store, &missing), Err(DynrsStatus::Empty));
        assert_eq!(read_value(store, &missing), Ok(None));

        ngenrs_kv_close(store);
    }

    #[test]
    fn values_survive_close_and_a_reopen() {
        let file = TempDbFile::new("reopen");
        let path = file.to_cstring();

        let store = open_store(&path);
        let key = CString::new("k").unwrap();
        let value = CString::new("kept").unwrap();
        assert_eq!(
            ngenrs_kv_write_string(store, key.as_ptr(), value.as_ptr()),
            DynrsStatus::Ok
        );
        ngenrs_kv_close(store);

        let store = open_store(&path);
        assert_eq!(read_value(store, &key), Ok(Some("kept".to_string())));
        ngenrs_kv_close(store);
    }

    #[test]
    fn open_rejects_an_empty_path() {
        let path = CString::new("").unwrap();
        let mut store = std::ptr::null_mut();
        assert_eq!(
            ngenrs_kv_open(path.as_ptr(), &mut store),
            DynrsStatus::Failed,
            "the path is readable, the store is not"
        );
        assert!(store.is_null());
    }

    #[test]
    fn contains_remove_all_keys_and_clear_through_the_c_abi() {
        let file = TempDbFile::new("collections");
        let path = file.to_cstring();
        let store = open_store(&path);

        let a = CString::new("a").unwrap();
        let b = CString::new("b").unwrap();
        let missing = CString::new("missing").unwrap();
        let value = CString::new("value").unwrap();
        assert_eq!(
            ngenrs_kv_write_string(store, a.as_ptr(), value.as_ptr()),
            DynrsStatus::Ok
        );
        assert_eq!(write_int(store, &b, 2), DynrsStatus::Ok);

        assert_eq!(contains(store, &a), Ok(true));
        assert_eq!(contains(store, &b), Ok(true));
        // "No" is `Ok` with the predicate false, not `Empty`: the store answered.
        let mut present = true;
        assert_eq!(
            ngenrs_kv_contains(store, missing.as_ptr(), &mut present),
            DynrsStatus::Ok
        );
        assert!(!present);

        // All keys come back sorted, behind the opaque handle.
        assert_eq!(read_keys_text(store), ["a", "b"]);

        // An index past the end is `Empty` rather than a null entry that reads like a key.
        let mut keys = std::ptr::null_mut();
        assert_eq!(ngenrs_kv_all_keys(store, &mut keys), DynrsStatus::Ok);
        let mut key_ptr = std::ptr::null();
        let mut key_len = 0usize;
        assert_eq!(
            ngenrs_kv_key_at(keys, 2, &mut key_ptr, &mut key_len),
            DynrsStatus::Empty
        );
        assert_eq!(
            ngenrs_kv_key_at(keys, usize::MAX, &mut key_ptr, &mut key_len),
            DynrsStatus::Empty
        );
        ngenrs_kv_free_keys(keys);

        assert_eq!(ngenrs_kv_remove(store, a.as_ptr()), DynrsStatus::Ok);
        assert_eq!(contains(store, &a), Ok(false));
        // Removing it again reports that there was nothing left to remove.
        assert_eq!(ngenrs_kv_remove(store, a.as_ptr()), DynrsStatus::Empty);

        assert_eq!(ngenrs_kv_clear(store), DynrsStatus::Ok);
        assert_eq!(contains(store, &b), Ok(false));

        // A store with no keys still lists successfully: an empty list is a value, and it used to
        // be the same null a failed query produced.
        assert!(read_keys_text(store).is_empty());

        ngenrs_kv_close(store);
    }

    #[test]
    fn an_empty_key_is_refused_through_the_c_abi() {
        let file = TempDbFile::new("empty_key");
        let path = file.to_cstring();
        let store = open_store(&path);

        let empty = CString::new("").unwrap();
        let value = CString::new("v").unwrap();
        assert_eq!(write_int(store, &empty, 1), DynrsStatus::InvalidArgument);
        assert_eq!(
            ngenrs_kv_write_float(store, empty.as_ptr(), 1.5),
            DynrsStatus::InvalidArgument
        );
        assert_eq!(
            ngenrs_kv_write_string(store, empty.as_ptr(), value.as_ptr()),
            DynrsStatus::InvalidArgument
        );
        assert_eq!(read_int(store, &empty), Err(DynrsStatus::InvalidArgument));
        assert_eq!(
            read_value(store, &empty),
            Err(DynrsStatus::InvalidArgument),
            "an empty key is the same invalid argument for a read as for a write"
        );
        assert_eq!(contains(store, &empty), Err(DynrsStatus::InvalidArgument));
        assert_eq!(
            ngenrs_kv_remove(store, empty.as_ptr()),
            DynrsStatus::InvalidArgument
        );

        ngenrs_kv_close(store);
    }

    /// A key or value the C ABI cannot express as a C string, but that the store can hold: a NUL
    /// byte is legal in a Rust `&str`. The store is reached through the core API because a C caller
    /// could not have written such a key in the first place; what is checked here is that reading
    /// one back does not lose it.
    #[test]
    fn a_key_with_a_nul_is_listed_rather_than_dropped() {
        let file = TempDbFile::new("nul_key");
        let path = file.to_cstring();

        // Write it through the core layer, then list it through the C ABI.
        let core = KV::open(&file.path).expect("store opens");
        core.write_string("a\0b", "value")
            .expect("the key is stored");
        drop(core);

        let store = open_store(&path);
        let keys = read_keys_text(store);
        assert_eq!(
            keys,
            ["a\0b"],
            "a key holding a NUL survives the listing instead of becoming a null entry"
        );
        ngenrs_kv_close(store);
    }
}
