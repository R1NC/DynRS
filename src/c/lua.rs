use crate::DynrsStatus;
use crate::c::util::{box_into_raw_new, bytes_to_c, cstr_to_rust, rust_to_cstr_lossy};
use crate::core::lua::LuaBridge;
use std::ffi::{c_char, c_void};

/// Borrows a bridge handle, or reports it as invalid.
fn bridge_ref<'a>(bridge: *const c_void) -> Result<&'a LuaBridge, DynrsStatus> {
    unsafe { (bridge as *const LuaBridge).as_ref() }.ok_or(DynrsStatus::InvalidHandle)
}

/// Creates a bridge, handing the handle back through `out`.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_lua_bridge_init(out: *mut *mut c_void) -> DynrsStatus {
    ffi_return! {
        if !out.is_null() {
            unsafe { *out = std::ptr::null_mut() };
        }
        if out.is_null() {
            return DynrsStatus::InvalidArgument;
        }
        match LuaBridge::new() {
            Ok(bridge) => {
                unsafe { *out = box_into_raw_new(bridge) as *mut c_void };
                DynrsStatus::Ok
            }
            Err(_) => DynrsStatus::Failed,
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_lua_bridge_release(bridge: *mut c_void) {
    ffi_return! {
        if !bridge.is_null() {
            unsafe { drop(Box::from_raw(bridge as *mut LuaBridge)) };
        }
    }
}

/// Loads a script from a file.
///
/// The failure text is reported through `err_out`, because a script that does not load fails for
/// reasons the caller has to act on — a missing file, a syntax error with a line number — and the
/// earlier shape had no channel for it at all: `false` was everything the caller could see.
///
/// Release a non-null `err_out` with `ngenrs_free_cstr`.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_lua_load_file(
    bridge: *mut c_void,
    path: *const c_char,
    err_out: *mut *mut c_char,
) -> DynrsStatus {
    ffi_return! {
        if !err_out.is_null() {
            unsafe { *err_out = std::ptr::null_mut() };
        }
        let bridge = match bridge_ref(bridge) {
            Ok(bridge) => bridge,
            Err(status) => return status,
        };
        let Some(path) = cstr_to_rust(path) else {
            return DynrsStatus::InvalidArgument;
        };
        // A panic raised anywhere below this point — inside mlua, or by a script callback that
        // touches a broken invariant — is caught by the guard instead of unwinding into the host.
        match bridge.load_file(path) {
            Ok(()) => DynrsStatus::Ok,
            Err(message) => {
                if !err_out.is_null() {
                    unsafe { *err_out = rust_to_cstr_lossy(message) };
                }
                DynrsStatus::Failed
            }
        }
    }
}

/// Loads a script from text. See [`ngenrs_lua_load_file`] for `err_out`.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_lua_load_string(
    bridge: *mut c_void,
    script: *const c_char,
    err_out: *mut *mut c_char,
) -> DynrsStatus {
    ffi_return! {
        if !err_out.is_null() {
            unsafe { *err_out = std::ptr::null_mut() };
        }
        let bridge = match bridge_ref(bridge) {
            Ok(bridge) => bridge,
            Err(status) => return status,
        };
        let Some(script) = cstr_to_rust(script) else {
            return DynrsStatus::InvalidArgument;
        };
        match bridge.load_string(script) {
            Ok(()) => DynrsStatus::Ok,
            Err(message) => {
                if !err_out.is_null() {
                    unsafe { *err_out = rust_to_cstr_lossy(message) };
                }
                DynrsStatus::Failed
            }
        }
    }
}

/// Calls a global Lua function with one string argument.
///
/// `result_out` receives the bytes of what the function returned and `result_len_out` their length,
/// because a script may return a string containing a NUL byte — as a C string that came back as a
/// null pointer, which is the same thing "the call failed" reports. `err_out` stays a C string:
/// error messages are produced by this library rather than by the caller's data.
///
/// Release a non-null `result_out` with `ngenrs_free_bytes(ptr)` and a non-null `err_out` with
/// `ngenrs_free_cstr`.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_lua_call_function(
    bridge: *mut c_void,
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
        let bridge = match bridge_ref(bridge) {
            Ok(bridge) => bridge,
            Err(status) => return status,
        };
        let Some(func_name) = cstr_to_rust(func_name) else {
            return DynrsStatus::InvalidArgument;
        };
        // The argument is optional: a null pointer is the empty call, which is what the earlier
        // shape did with `cstr_to_rust` returning `None` for null.
        let arg = if arg.is_null() {
            ""
        } else {
            match cstr_to_rust(arg) {
                Some(arg) => arg,
                None => return DynrsStatus::InvalidArgument,
            }
        };

        // mlua catches a panic inside a Lua callback only to re-raise it with `resume_unwind`, so
        // it arrives here as a panic rather than as an error value; this is the last place it can
        // be stopped before the C boundary.
        match bridge.call_function(func_name, arg) {
            Ok(result) => {
                if !result_out.is_null() {
                    unsafe { *result_out = bytes_to_c(result.into_bytes(), result_len_out) };
                }
                DynrsStatus::Ok
            }
            Err(e) => {
                if !err_out.is_null() {
                    unsafe { *err_out = rust_to_cstr_lossy(e.to_string()) };
                }
                DynrsStatus::Failed
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::c::util::{ngenrs_free_bytes, ngenrs_free_cstr};
    use std::ffi::{CStr, CString};
    use std::path::PathBuf;

    /// The script the tests below load: its functions report what the timers did.
    const SCRIPT: &str = r#"
        local fired = 0

        function on_fire() fired = fired + 1 end

        local handle = addTimer(0, "on_fire")

        function count() return tostring(fired) end
        function poll() pollTimers() return tostring(fired) end
        function stop() removeTimer(handle) return "ok" end
    "#;

    /// Creates a bridge through the C ABI, asserting that it worked.
    fn new_bridge() -> *mut c_void {
        let mut bridge = std::ptr::null_mut();
        assert_eq!(
            ngenrs_lua_bridge_init(&mut bridge),
            DynrsStatus::Ok,
            "the bridge is created"
        );
        bridge
    }

    /// Loads a script and returns whatever the bridge said about the failure.
    fn load_string(bridge: *mut c_void, script: &CString) -> Result<(), String> {
        let mut error = std::ptr::null_mut();
        match ngenrs_lua_load_string(bridge, script.as_ptr(), &mut error) {
            DynrsStatus::Ok => Ok(()),
            _ => {
                let text = unsafe { CStr::from_ptr(error) }
                    .to_str()
                    .unwrap()
                    .to_string();
                ngenrs_free_cstr(error);
                Err(text)
            }
        }
    }

    /// Loads a script file and returns whatever the bridge said about the failure.
    fn load_file(bridge: *mut c_void, path: &CString) -> Result<(), String> {
        let mut error = std::ptr::null_mut();
        match ngenrs_lua_load_file(bridge, path.as_ptr(), &mut error) {
            DynrsStatus::Ok => Ok(()),
            _ => {
                let text = unsafe { CStr::from_ptr(error) }
                    .to_str()
                    .unwrap()
                    .to_string();
                ngenrs_free_cstr(error);
                Err(text)
            }
        }
    }

    /// Calls `name` through the C ABI, giving back the answer or the message that came with the
    /// failure, and releasing whichever buffer the bridge handed out.
    fn call(bridge: *mut c_void, name: &str) -> Result<String, String> {
        let name = CString::new(name).unwrap();
        let arg = CString::new("").unwrap();
        let mut result = std::ptr::null_mut();
        let mut result_len = 0usize;
        let mut error = std::ptr::null_mut();

        let status = ngenrs_lua_call_function(
            bridge,
            name.as_ptr(),
            arg.as_ptr(),
            &mut result,
            &mut result_len,
            &mut error,
        );

        if status == DynrsStatus::Ok {
            let bytes = unsafe { std::slice::from_raw_parts(result, result_len) }.to_vec();
            ngenrs_free_bytes(result);
            Ok(String::from_utf8(bytes).expect("the script returns text"))
        } else {
            let text = unsafe { CStr::from_ptr(error) }
                .to_str()
                .unwrap()
                .to_string();
            ngenrs_free_cstr(error);
            Err(text)
        }
    }

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("dynrs_lua_{}_{name}", std::process::id()))
    }

    #[test]
    fn a_script_and_its_timers_round_trip_through_the_c_abi() {
        let bridge = new_bridge();

        let script = CString::new(SCRIPT).unwrap();
        load_string(bridge, &script).expect("the script loads");

        // Registering a timer does not run it; polling does.
        assert_eq!(call(bridge, "count").unwrap(), "0");
        assert_eq!(call(bridge, "poll").unwrap(), "1");
        assert_eq!(call(bridge, "stop").unwrap(), "ok");
        assert_eq!(call(bridge, "poll").unwrap(), "1");

        ngenrs_lua_bridge_release(bridge);
    }

    #[test]
    fn a_script_can_be_loaded_from_a_file() {
        let bridge = new_bridge();
        let path = temp_path("script.lua");
        std::fs::write(&path, "function answer() return \"42\" end\n")
            .expect("the script is written");

        let loaded = CString::new(path.to_str().unwrap()).unwrap();
        load_file(bridge, &loaded).expect("the file loads");
        assert_eq!(call(bridge, "answer").unwrap(), "42");

        // A path that is not there is reported, not panicked on, and the message says which path.
        let missing = CString::new(temp_path("missing.lua").to_str().unwrap()).unwrap();
        let message = load_file(bridge, &missing).unwrap_err();
        assert!(!message.is_empty(), "the failure has a message: {message}");

        std::fs::remove_file(&path).expect("the file is removed again");
        ngenrs_lua_bridge_release(bridge);
    }

    #[test]
    fn a_broken_script_a_missing_function_and_null_arguments_are_reported() {
        let bridge = new_bridge();

        let broken = CString::new("function oops( end").unwrap();
        let message = load_string(bridge, &broken).unwrap_err();
        assert!(
            !message.is_empty(),
            "a compile failure has a message: {message}"
        );

        let error = call(bridge, "not_a_function").unwrap_err();
        assert!(!error.is_empty(), "a missing function has a message");

        // A null bridge, script, path, function name or argument is refused rather than read.
        let name = CString::new("count").unwrap();
        assert_eq!(
            ngenrs_lua_load_string(std::ptr::null_mut(), broken.as_ptr(), std::ptr::null_mut()),
            DynrsStatus::InvalidHandle
        );
        assert_eq!(
            ngenrs_lua_load_string(bridge, std::ptr::null(), std::ptr::null_mut()),
            DynrsStatus::InvalidArgument
        );
        assert_eq!(
            ngenrs_lua_load_file(std::ptr::null_mut(), broken.as_ptr(), std::ptr::null_mut()),
            DynrsStatus::InvalidHandle
        );
        assert_eq!(
            ngenrs_lua_load_file(bridge, std::ptr::null(), std::ptr::null_mut()),
            DynrsStatus::InvalidArgument
        );
        assert_eq!(
            ngenrs_lua_call_function(
                std::ptr::null_mut(),
                name.as_ptr(),
                name.as_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut()
            ),
            DynrsStatus::InvalidHandle
        );
        assert_eq!(
            ngenrs_lua_call_function(
                bridge,
                std::ptr::null(),
                name.as_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut()
            ),
            DynrsStatus::InvalidArgument
        );
        // A null argument is the empty call, not a rejected one.
        assert_eq!(
            ngenrs_lua_call_function(
                bridge,
                name.as_ptr(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut()
            ),
            DynrsStatus::Failed,
            "the call ran and the script refused it"
        );

        ngenrs_lua_bridge_release(std::ptr::null_mut());
        ngenrs_lua_bridge_release(bridge);
    }
}
