use crate::DynrsStatus;
use crate::c::util::{
    box_into_raw_new, bytes_to_c, cbytes_to_rust, cstr_to_rust, ngenrs_free_ptr, rust_to_cstr_lossy,
};
use crate::core::qjs::JSBridge;
use libc::{c_char, c_void};

/// Borrows a bridge handle, or reports it as invalid.
fn bridge_ref<'a>(handle: *const c_void) -> Result<&'a JSBridge, DynrsStatus> {
    unsafe { (handle as *const JSBridge).as_ref() }.ok_or(DynrsStatus::InvalidHandle)
}

/// Clears an error slot and returns the text a load reported.
///
/// `err_out` used to be left untouched when an argument was rejected, so a caller that passed a
/// real slot had to initialise it to null itself or read whatever was there before. Every load now
/// writes the slot on every path: a message when the load failed, null when it did not.
fn load_failed(err_out: *mut *mut c_char, message: String) -> DynrsStatus {
    if !err_out.is_null() {
        unsafe { *err_out = rust_to_cstr_lossy(message) };
    }
    DynrsStatus::Failed
}

/// Runs a load operation and reports what happened.
fn _ngenrs_qjs_load<T, F>(
    handle: *mut c_void,
    input: T,
    err_out: *mut *mut c_char,
    operation: F,
) -> DynrsStatus
where
    T: Copy,
    F: FnOnce(&JSBridge, T) -> Result<(), String>,
{
    ffi_return! {
        if !err_out.is_null() {
            unsafe { *err_out = std::ptr::null_mut() };
        }
        let bridge = match bridge_ref(handle) {
            Ok(bridge) => bridge,
            Err(status) => return status,
        };
        match operation(bridge, input) {
            Ok(()) => DynrsStatus::Ok,
            Err(message) => load_failed(err_out, message),
        }
    }
}

/// Creates a bridge, handing the handle back through `out`.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_qjs_init(out: *mut *mut c_void) -> DynrsStatus {
    ffi_return! {
        if !out.is_null() {
            unsafe { *out = std::ptr::null_mut() };
        }
        if out.is_null() {
            return DynrsStatus::InvalidArgument;
        }
        match JSBridge::new() {
            Ok(bridge) => {
                unsafe { *out = box_into_raw_new(bridge) as *mut c_void };
                DynrsStatus::Ok
            }
            Err(_) => DynrsStatus::Failed,
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_qjs_load_script_file(
    handle: *mut c_void,
    path: *const c_char,
    is_module: bool,
    err_out: *mut *mut c_char,
) -> DynrsStatus {
    ffi_return! {
        if !err_out.is_null() {
            unsafe { *err_out = std::ptr::null_mut() };
        }
        let Some(path_str) = cstr_to_rust(path) else {
            return DynrsStatus::InvalidArgument;
        };
        _ngenrs_qjs_load(
            handle,
            (path_str, is_module),
            err_out,
            |bridge, (path, is_module)| bridge.load_script_file(path, is_module),
        )
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_qjs_load_script_content(
    handle: *mut c_void,
    script: *const c_char,
    is_module: bool,
    err_out: *mut *mut c_char,
) -> DynrsStatus {
    ffi_return! {
        if !err_out.is_null() {
            unsafe { *err_out = std::ptr::null_mut() };
        }
        let Some(script_str) = cstr_to_rust(script) else {
            return DynrsStatus::InvalidArgument;
        };
        _ngenrs_qjs_load(
            handle,
            (script_str, is_module),
            err_out,
            |bridge, (script, is_module)| bridge.load_script_content(script, is_module),
        )
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_qjs_load_bytecode_file(
    handle: *mut c_void,
    path: *const c_char,
    err_out: *mut *mut c_char,
) -> DynrsStatus {
    ffi_return! {
        if !err_out.is_null() {
            unsafe { *err_out = std::ptr::null_mut() };
        }
        let Some(path_str) = cstr_to_rust(path) else {
            return DynrsStatus::InvalidArgument;
        };
        _ngenrs_qjs_load(handle, path_str, err_out, |bridge, path| {
            bridge.load_bytecode_file(path)
        })
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_qjs_load_bytecode_content(
    handle: *mut c_void,
    bytecode: *const u8,
    length: usize,
    err_out: *mut *mut c_char,
) -> DynrsStatus {
    ffi_return! {
        if !err_out.is_null() {
            unsafe { *err_out = std::ptr::null_mut() };
        }
        let Some(bytecode_slice) = cbytes_to_rust(bytecode, length) else {
            return DynrsStatus::InvalidArgument;
        };
        _ngenrs_qjs_load(handle, bytecode_slice, err_out, |bridge, bytecode| {
            bridge.load_bytecode_content(bytecode)
        })
    }
}

/// Calls a global function with one string argument.
///
/// `result_out` receives the bytes of what the function returned and `result_len_out` their length,
/// because a script may return a string containing a NUL byte — as a C string that came back as a
/// null pointer, which is the same thing "the call failed" reports. `err_out` stays a C string:
/// error messages are produced by this library rather than by the caller's data.
///
/// Release a non-null `result_out` with `ngenrs_free_bytes(ptr)` and a non-null `err_out` with
/// `ngenrs_free_cstr`.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_qjs_call_function(
    handle: *mut c_void,
    func_name: *const c_char,
    arg: *const c_char,
    result_out: *mut *mut u8,
    result_len_out: *mut usize,
    err_out: *mut *mut c_char,
) -> DynrsStatus {
    ffi_return! {
        if !result_out.is_null() {
            unsafe { *result_out = std::ptr::null_mut() };
        }
        if !result_len_out.is_null() {
            unsafe { *result_len_out = 0 };
        }
        if !err_out.is_null() {
            unsafe { *err_out = std::ptr::null_mut() };
        }
        let bridge = match bridge_ref(handle) {
            Ok(bridge) => bridge,
            Err(status) => return status,
        };
        let Some(func_name_str) = cstr_to_rust(func_name) else {
            return DynrsStatus::InvalidArgument;
        };
        // The argument is optional: a null pointer is the empty call.
        let arg_str = if arg.is_null() {
            ""
        } else {
            match cstr_to_rust(arg) {
                Some(arg) => arg,
                None => return DynrsStatus::InvalidArgument,
            }
        };

        match bridge.call_function(func_name_str, arg_str) {
            Ok(result) => {
                if !result_out.is_null() {
                    unsafe { *result_out = bytes_to_c(result.into_bytes(), result_len_out) };
                }
                DynrsStatus::Ok
            }
            Err(message) => load_failed(err_out, message),
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_qjs_release(handle: *mut c_void) {
    ffi_return! {
        if !handle.is_null() {
            ngenrs_free_ptr(handle as *mut JSBridge);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::c::util::{ngenrs_free_bytes, ngenrs_free_cstr};
    use std::ffi::{CStr, CString};
    use std::path::PathBuf;

    /// The script the tests below load: one function that answers, and one that throws.
    const SCRIPT: &str = r#"
        function greet(name) { return "hi " + name }
        function boom() { throw new Error("nope") }
    "#;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("dynrs_qjs_{}_{name}", std::process::id()))
    }

    /// Creates a bridge through the C ABI, asserting that it worked.
    fn new_bridge() -> *mut c_void {
        let mut handle = std::ptr::null_mut();
        assert_eq!(
            ngenrs_qjs_init(&mut handle),
            DynrsStatus::Ok,
            "the bridge is created"
        );
        handle
    }

    /// Reads the string the bridge handed out, and frees it so that no test leaks one.
    fn take(string: *mut c_char) -> String {
        if string.is_null() {
            return String::new();
        }
        let text = unsafe { CStr::from_ptr(string) }
            .to_str()
            .unwrap()
            .to_string();
        ngenrs_free_cstr(string);
        text
    }

    /// Loads `script` through the C ABI; the failure carries the message the bridge wrote.
    fn load_script(handle: *mut c_void, script: &str, is_module: bool) -> Result<(), String> {
        let script = CString::new(script).unwrap();
        let mut error = std::ptr::null_mut();
        match ngenrs_qjs_load_script_content(handle, script.as_ptr(), is_module, &mut error) {
            DynrsStatus::Ok => Ok(()),
            _ => Err(take(error)),
        }
    }

    /// Calls `name` with `arg` through the C ABI.
    fn call(handle: *mut c_void, name: &str, arg: &str) -> Result<String, String> {
        let name = CString::new(name).unwrap();
        let arg = CString::new(arg).unwrap();
        let mut result = std::ptr::null_mut();
        let mut result_len = 0usize;
        let mut error = std::ptr::null_mut();
        match ngenrs_qjs_call_function(
            handle,
            name.as_ptr(),
            arg.as_ptr(),
            &mut result,
            &mut result_len,
            &mut error,
        ) {
            DynrsStatus::Ok => {
                let bytes = unsafe { std::slice::from_raw_parts(result, result_len) }.to_vec();
                ngenrs_free_bytes(result);
                Ok(String::from_utf8(bytes).expect("the script returns text"))
            }
            _ => Err(take(error)),
        }
    }

    #[test]
    fn a_script_round_trips_through_the_c_abi() {
        let handle = new_bridge();

        load_script(handle, SCRIPT, false).expect("the script loads");
        assert_eq!(call(handle, "greet", "there").unwrap(), "hi there");

        // A throw reaches the caller as the message of the error.
        let error = call(handle, "boom", "").unwrap_err();
        assert!(error.contains("nope"), "{error}");

        // So does a function that is not there: reading a missing property yields `undefined`
        // without an exception, so the type error appears when it is called.
        let error = call(handle, "not_a_function", "").unwrap_err();
        assert!(error.contains("not a function"), "{error}");

        ngenrs_qjs_release(handle);
    }

    #[test]
    fn a_module_and_a_file_script_load_as_well() {
        let handle = new_bridge();

        // A module has a scope of its own, so its function goes on the global object to be
        // reachable from `call_function`. The argument arrives as a string, so `+` concatenates.
        load_script(
            handle,
            "globalThis.from_module = function (x) { return 'module ' + x };",
            true,
        )
        .expect("the module loads");
        assert_eq!(call(handle, "from_module", "41").unwrap(), "module 41");

        // A script that does not compile is reported.
        let error = load_script(handle, "function broken( {", false).unwrap_err();
        assert!(
            !error.is_empty(),
            "a script that does not compile has a message"
        );

        let path = temp_path("script.js");
        std::fs::write(&path, "function from_file() { return \"file\" }")
            .expect("the script is written");
        let file = CString::new(path.to_str().unwrap()).unwrap();
        assert_eq!(
            ngenrs_qjs_load_script_file(handle, file.as_ptr(), false, std::ptr::null_mut()),
            DynrsStatus::Ok,
            "the file loads"
        );
        assert_eq!(call(handle, "from_file", "").unwrap(), "file");

        // A path that is not there is reported, not panicked on.
        let missing = CString::new(temp_path("missing.js").to_str().unwrap()).unwrap();
        let mut error = std::ptr::null_mut();
        assert_eq!(
            ngenrs_qjs_load_script_file(handle, missing.as_ptr(), false, &mut error),
            DynrsStatus::Failed
        );
        assert!(!take(error).is_empty(), "a missing file has a message");

        std::fs::remove_file(&path).expect("the file is removed again");
        ngenrs_qjs_release(handle);
    }

    #[test]
    fn bytecode_that_cannot_be_read_and_null_arguments_are_reported() {
        let handle = new_bridge();

        let garbage = [0u8, 1, 2, 3, 4, 5, 6, 7];
        let mut error = std::ptr::null_mut();
        assert_eq!(
            ngenrs_qjs_load_bytecode_content(handle, garbage.as_ptr(), garbage.len(), &mut error),
            DynrsStatus::Failed
        );
        assert!(
            !take(error).is_empty(),
            "bytecode that cannot be read has a message"
        );

        let missing = CString::new(temp_path("missing.qbc").to_str().unwrap()).unwrap();
        let mut error = std::ptr::null_mut();
        assert_eq!(
            ngenrs_qjs_load_bytecode_file(handle, missing.as_ptr(), &mut error),
            DynrsStatus::Failed
        );
        assert!(
            !take(error).is_empty(),
            "a missing bytecode file has a message"
        );

        // A null handle, script, path, name or argument is refused rather than read.
        let script = CString::new("1").unwrap();
        let name = CString::new("greet").unwrap();
        assert_eq!(
            ngenrs_qjs_load_script_content(
                std::ptr::null_mut(),
                script.as_ptr(),
                false,
                std::ptr::null_mut()
            ),
            DynrsStatus::InvalidHandle
        );
        assert_eq!(
            ngenrs_qjs_load_script_content(handle, std::ptr::null(), false, std::ptr::null_mut()),
            DynrsStatus::InvalidArgument
        );
        assert_eq!(
            ngenrs_qjs_load_script_file(
                std::ptr::null_mut(),
                script.as_ptr(),
                false,
                std::ptr::null_mut()
            ),
            DynrsStatus::InvalidHandle
        );
        assert_eq!(
            ngenrs_qjs_load_script_file(handle, std::ptr::null(), false, std::ptr::null_mut()),
            DynrsStatus::InvalidArgument
        );
        assert_eq!(
            ngenrs_qjs_load_bytecode_content(
                std::ptr::null_mut(),
                garbage.as_ptr(),
                garbage.len(),
                std::ptr::null_mut()
            ),
            DynrsStatus::InvalidHandle
        );
        assert_eq!(
            ngenrs_qjs_load_bytecode_content(handle, std::ptr::null(), 0, std::ptr::null_mut()),
            DynrsStatus::InvalidArgument
        );
        assert_eq!(
            ngenrs_qjs_load_bytecode_file(handle, std::ptr::null(), std::ptr::null_mut()),
            DynrsStatus::InvalidArgument
        );
        assert_eq!(
            ngenrs_qjs_call_function(
                std::ptr::null_mut(),
                name.as_ptr(),
                script.as_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut()
            ),
            DynrsStatus::InvalidHandle
        );
        assert_eq!(
            ngenrs_qjs_call_function(
                handle,
                std::ptr::null(),
                script.as_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut()
            ),
            DynrsStatus::InvalidArgument
        );
        // A null argument is the empty call rather than a rejected one: it reaches the script with
        // the empty string, which is what the Lua entry point does as well.
        load_script(handle, "function greet(x) { return 'hi ' + x }", false)
            .expect("the script loads");
        let name = CString::new("greet").unwrap();
        let mut result = std::ptr::null_mut();
        let mut result_len = 0usize;
        assert_eq!(
            ngenrs_qjs_call_function(
                handle,
                name.as_ptr(),
                std::ptr::null(),
                &mut result,
                &mut result_len,
                std::ptr::null_mut()
            ),
            DynrsStatus::Ok,
            "greet with an empty argument answers"
        );
        assert_eq!(
            std::str::from_utf8(unsafe { std::slice::from_raw_parts(result, result_len) }).unwrap(),
            "hi "
        );
        ngenrs_free_bytes(result);

        ngenrs_qjs_release(std::ptr::null_mut());
        ngenrs_qjs_release(handle);
    }
}
