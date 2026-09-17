use crate::c::util::{
    box_into_raw_new, cbytes_to_rust, cstr_to_rust, ngenrs_free_ptr, rust_to_cstr,
};
use crate::core::qjs::JSBridge;
use libc::{c_char, c_void};

/// Creates a QuickJS bridge, or returns null if the engine could not be started.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_qjs_init() -> *mut c_void {
    match JSBridge::new() {
        Ok(bridge) => box_into_raw_new(bridge) as *mut c_void,
        Err(_) => std::ptr::null_mut(),
    }
}

fn _ngenrs_qjs_load<T, F>(
    handle: *mut c_void,
    input: T,
    err_out: *mut *mut c_char,
    operation: F,
) -> bool
where
    T: Copy,
    F: FnOnce(&JSBridge, T) -> Result<(), String>,
{
    if handle.is_null() {
        return false;
    }

    let bridge = unsafe { &*(handle as *mut JSBridge) };
    match operation(bridge, input) {
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
pub extern "C" fn ngenrs_qjs_load_script_file(
    handle: *mut c_void,
    path: *const c_char,
    is_module: bool,
    err_out: *mut *mut c_char,
) -> bool {
    if path.is_null() {
        return false;
    }
    let path_str = match cstr_to_rust(path) {
        Some(s) => s,
        None => return false,
    };
    _ngenrs_qjs_load(
        handle,
        (path_str, is_module),
        err_out,
        |bridge, (path, is_module)| bridge.load_script_file(path, is_module),
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_qjs_load_script_content(
    handle: *mut c_void,
    script: *const c_char,
    is_module: bool,
    err_out: *mut *mut c_char,
) -> bool {
    if script.is_null() {
        return false;
    }
    let script_str = match cstr_to_rust(script) {
        Some(s) => s,
        None => return false,
    };
    _ngenrs_qjs_load(
        handle,
        (script_str, is_module),
        err_out,
        |bridge, (script, is_module)| bridge.load_script_content(script, is_module),
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_qjs_load_bytecode_file(
    handle: *mut c_void,
    path: *const c_char,
    err_out: *mut *mut c_char,
) -> bool {
    if path.is_null() {
        return false;
    }
    let path_str = match cstr_to_rust(path) {
        Some(s) => s,
        None => return false,
    };
    _ngenrs_qjs_load(handle, path_str, err_out, |bridge, path| {
        bridge.load_bytecode_file(path)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_qjs_load_bytecode_content(
    handle: *mut c_void,
    bytecode: *const u8,
    length: usize,
    err_out: *mut *mut c_char,
) -> bool {
    if bytecode.is_null() {
        return false;
    }
    let bytecode_slice = match cbytes_to_rust(bytecode, length) {
        Some(slice) => slice,
        None => return false,
    };
    _ngenrs_qjs_load(handle, bytecode_slice, err_out, |bridge, bytecode| {
        bridge.load_bytecode_content(bytecode)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_qjs_call_function(
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

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_qjs_release(handle: *mut c_void) {
    if !handle.is_null() {
        ngenrs_free_ptr(handle as *mut JSBridge);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::c::util::ngenrs_free_cstr;
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
        if ngenrs_qjs_load_script_content(handle, script.as_ptr(), is_module, &mut error) {
            Ok(())
        } else {
            Err(take(error))
        }
    }

    /// Calls `name` with `arg` through the C ABI.
    fn call(handle: *mut c_void, name: &str, arg: &str) -> Result<String, String> {
        let name = CString::new(name).unwrap();
        let arg = CString::new(arg).unwrap();
        let mut result = std::ptr::null_mut();
        let mut error = std::ptr::null_mut();
        if ngenrs_qjs_call_function(handle, name.as_ptr(), arg.as_ptr(), &mut result, &mut error) {
            Ok(take(result))
        } else {
            Err(take(error))
        }
    }

    #[test]
    fn a_script_round_trips_through_the_c_abi() {
        let handle = ngenrs_qjs_init();
        assert!(!handle.is_null(), "the bridge is created");

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
        let handle = ngenrs_qjs_init();

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
        assert!(
            ngenrs_qjs_load_script_file(handle, file.as_ptr(), false, std::ptr::null_mut()),
            "the file loads"
        );
        assert_eq!(call(handle, "from_file", "").unwrap(), "file");

        // A path that is not there is reported, not panicked on.
        let missing = CString::new(temp_path("missing.js").to_str().unwrap()).unwrap();
        let mut error = std::ptr::null_mut();
        assert!(!ngenrs_qjs_load_script_file(
            handle,
            missing.as_ptr(),
            false,
            &mut error
        ));
        assert!(!take(error).is_empty(), "a missing file has a message");

        std::fs::remove_file(&path).expect("the file is removed again");
        ngenrs_qjs_release(handle);
    }

    #[test]
    fn bytecode_that_cannot_be_read_and_null_arguments_are_reported() {
        let handle = ngenrs_qjs_init();

        let garbage = [0u8, 1, 2, 3, 4, 5, 6, 7];
        let mut error = std::ptr::null_mut();
        assert!(!ngenrs_qjs_load_bytecode_content(
            handle,
            garbage.as_ptr(),
            garbage.len(),
            &mut error
        ));
        assert!(
            !take(error).is_empty(),
            "bytecode that cannot be read has a message"
        );

        let missing = CString::new(temp_path("missing.qbc").to_str().unwrap()).unwrap();
        let mut error = std::ptr::null_mut();
        assert!(!ngenrs_qjs_load_bytecode_file(
            handle,
            missing.as_ptr(),
            &mut error
        ));
        assert!(
            !take(error).is_empty(),
            "a missing bytecode file has a message"
        );

        // A null handle, script, path, name or argument is refused rather than read.
        let script = CString::new("1").unwrap();
        let name = CString::new("greet").unwrap();
        assert!(!ngenrs_qjs_load_script_content(
            std::ptr::null_mut(),
            script.as_ptr(),
            false,
            std::ptr::null_mut()
        ));
        assert!(!ngenrs_qjs_load_script_content(
            handle,
            std::ptr::null(),
            false,
            std::ptr::null_mut()
        ));
        assert!(!ngenrs_qjs_load_script_file(
            std::ptr::null_mut(),
            script.as_ptr(),
            false,
            std::ptr::null_mut()
        ));
        assert!(!ngenrs_qjs_load_script_file(
            handle,
            std::ptr::null(),
            false,
            std::ptr::null_mut()
        ));
        assert!(!ngenrs_qjs_load_bytecode_content(
            std::ptr::null_mut(),
            garbage.as_ptr(),
            garbage.len(),
            std::ptr::null_mut()
        ));
        assert!(!ngenrs_qjs_load_bytecode_content(
            handle,
            std::ptr::null(),
            0,
            std::ptr::null_mut()
        ));
        assert!(!ngenrs_qjs_load_bytecode_file(
            handle,
            std::ptr::null(),
            std::ptr::null_mut()
        ));
        assert!(!ngenrs_qjs_call_function(
            std::ptr::null_mut(),
            name.as_ptr(),
            script.as_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut()
        ));
        assert!(!ngenrs_qjs_call_function(
            handle,
            std::ptr::null(),
            script.as_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut()
        ));
        assert!(!ngenrs_qjs_call_function(
            handle,
            name.as_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut()
        ));

        ngenrs_qjs_release(std::ptr::null_mut());
        ngenrs_qjs_release(handle);
    }
}
