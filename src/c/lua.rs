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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::c::util::ngenrs_free_cstr;
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

    /// Calls `name` through the C ABI, giving back the answer or the message that came with the
    /// failure, and freeing whichever string the bridge handed out.
    fn call(bridge: *mut c_void, name: &str) -> Result<String, String> {
        let name = CString::new(name).unwrap();
        let arg = CString::new("").unwrap();
        let mut result = std::ptr::null_mut();
        let mut error = std::ptr::null_mut();

        let loaded =
            ngenrs_lua_call_function(bridge, name.as_ptr(), arg.as_ptr(), &mut result, &mut error);
        let (message, string) = if loaded {
            (Ok(()), result)
        } else {
            (Err(()), error)
        };
        let text = unsafe { CStr::from_ptr(string) }
            .to_str()
            .unwrap()
            .to_string();
        ngenrs_free_cstr(string);
        match message {
            Ok(()) => Ok(text),
            Err(()) => Err(text),
        }
    }

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("dynrs_lua_{}_{name}", std::process::id()))
    }

    #[test]
    fn a_script_and_its_timers_round_trip_through_the_c_abi() {
        let bridge = ngenrs_lua_bridge_init();
        assert!(!bridge.is_null(), "the bridge is created");

        let script = CString::new(SCRIPT).unwrap();
        assert!(
            ngenrs_lua_load_string(bridge, script.as_ptr()),
            "the script loads"
        );

        // Registering a timer does not run it; polling does.
        assert_eq!(call(bridge, "count").unwrap(), "0");
        assert_eq!(call(bridge, "poll").unwrap(), "1");
        assert_eq!(call(bridge, "stop").unwrap(), "ok");
        assert_eq!(call(bridge, "poll").unwrap(), "1");

        ngenrs_lua_bridge_release(bridge);
    }

    #[test]
    fn a_script_can_be_loaded_from_a_file() {
        let bridge = ngenrs_lua_bridge_init();
        let path = temp_path("script.lua");
        std::fs::write(&path, "function answer() return \"42\" end\n")
            .expect("the script is written");

        let loaded = CString::new(path.to_str().unwrap()).unwrap();
        assert!(
            ngenrs_lua_load_file(bridge, loaded.as_ptr()),
            "the file loads"
        );
        assert_eq!(call(bridge, "answer").unwrap(), "42");

        // A path that is not there is reported, not panicked on.
        let missing = CString::new(temp_path("missing.lua").to_str().unwrap()).unwrap();
        assert!(!ngenrs_lua_load_file(bridge, missing.as_ptr()));

        std::fs::remove_file(&path).expect("the file is removed again");
        ngenrs_lua_bridge_release(bridge);
    }

    #[test]
    fn a_broken_script_a_missing_function_and_null_arguments_are_reported() {
        let bridge = ngenrs_lua_bridge_init();

        let broken = CString::new("function oops( end").unwrap();
        assert!(
            !ngenrs_lua_load_string(bridge, broken.as_ptr()),
            "a script that does not compile is refused"
        );

        let error = call(bridge, "not_a_function").unwrap_err();
        assert!(!error.is_empty(), "a missing function has a message");

        // A null bridge, script, path, function name or argument is refused rather than read.
        let name = CString::new("count").unwrap();
        assert!(!ngenrs_lua_load_string(
            std::ptr::null_mut(),
            broken.as_ptr()
        ));
        assert!(!ngenrs_lua_load_string(bridge, std::ptr::null()));
        assert!(!ngenrs_lua_load_file(std::ptr::null_mut(), broken.as_ptr()));
        assert!(!ngenrs_lua_load_file(bridge, std::ptr::null()));
        assert!(!ngenrs_lua_call_function(
            std::ptr::null_mut(),
            name.as_ptr(),
            name.as_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut()
        ));
        assert!(!ngenrs_lua_call_function(
            bridge,
            std::ptr::null(),
            name.as_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut()
        ));
        assert!(!ngenrs_lua_call_function(
            bridge,
            name.as_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut()
        ));

        ngenrs_lua_bridge_release(std::ptr::null_mut());
        ngenrs_lua_bridge_release(bridge);
    }
}
