use crate::c::util::{cstr_to_rust, ngenrs_free_cstr, rust_to_cstr};
use crate::core::timer::{Timers, lock_timers};
use libquickjs_ng_sys::{
    JS_Call, JS_Eval, JS_FreeCString, JS_FreeValue, JS_GetException, JS_GetGlobalObject,
    JS_GetPropertyStr, JS_HasException, JS_NewCFunction2, JS_NewContext, JS_NewRuntime,
    JS_NewStringLen, JS_SetPropertyStr, JS_TAG_INT, JS_TAG_UNDEFINED, JS_Throw, JS_ToCStringLen2,
    JS_ToFloat64, JS_ToInt32, JSCFunction, JSCFunctionEnum_JS_CFUNC_generic_magic,
    JSCFunctionMagic, JSContext, JSRuntime, JSValue, JSValueUnion,
};
use std::collections::HashMap;
use std::ffi::CString;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// A function a script can call. It gets the context so it can convert its arguments and results.
type JsCallback = dyn Fn(*mut JSContext, Vec<JSValue>) -> Result<JSValue, String> + Send + Sync;

/// Registered script callbacks, keyed by the `magic` value carried by their function object.
///
/// QuickJS runs plain C functions that have no user data of their own, and `JS_SetContextOpaque`
/// holds a single pointer per context, so that id is what reaches the right closure.
static JS_CALLBACKS: OnceLock<Mutex<HashMap<i32, Arc<JsCallback>>>> = OnceLock::new();
static NEXT_CALLBACK_ID: AtomicI32 = AtomicI32::new(1);

fn js_callbacks() -> std::sync::MutexGuard<'static, HashMap<i32, Arc<JsCallback>>> {
    JS_CALLBACKS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap()
}

/// The C function QuickJS invokes for every registered script function.
unsafe extern "C" fn js_callback_trampoline(
    ctx: *mut JSContext,
    _this_val: JSValue,
    argc: i32,
    argv: *mut JSValue,
    magic: i32,
) -> JSValue {
    unsafe {
        let args = if argc > 0 {
            std::slice::from_raw_parts(argv, argc as usize).to_vec()
        } else {
            Vec::new()
        };

        // The registry lock is released before the callback runs, so a callback may register
        // another function of its own.
        let callback = js_callbacks().get(&magic).map(Arc::clone);

        let result = match callback {
            Some(callback) => callback(ctx, args),
            None => Err(format!("no JS callback is registered for id {magic}")),
        };

        match result {
            Ok(value) => value,
            Err(message) => {
                let message = rust_to_cstr(message);
                let thrown = JS_NewStringLen(ctx, message, libc::strlen(message));
                ngenrs_free_cstr(message);
                JS_Throw(ctx, thrown)
            }
        }
    }
}

/// `undefined`, built from the tags the bindings expose because the C macro is not available.
fn js_undefined() -> JSValue {
    JSValue {
        u: JSValueUnion { int32: 0 },
        tag: JS_TAG_UNDEFINED as i64,
    }
}

/// A small integer result.
fn js_int(value: i32) -> JSValue {
    JSValue {
        u: JSValueUnion { int32: value },
        tag: JS_TAG_INT as i64,
    }
}

/// Reads a number argument, refusing a missing or non-numeric one.
fn arg_f64(ctx: *mut JSContext, value: Option<&JSValue>) -> Result<f64, String> {
    let value = value.copied().ok_or("a number argument is required")?;
    let mut number = 0.0;
    if unsafe { JS_ToFloat64(ctx, &mut number, value) } < 0 {
        return Err("the argument is not a number".to_string());
    }
    Ok(number)
}

/// Reads an integer argument, refusing a missing or non-numeric one.
fn arg_i32(ctx: *mut JSContext, value: Option<&JSValue>) -> Result<i32, String> {
    let value = value.copied().ok_or("a handle argument is required")?;
    let mut number = 0;
    if unsafe { JS_ToInt32(ctx, &mut number, value) } < 0 {
        return Err("the handle is not a number".to_string());
    }
    Ok(number)
}

/// Reads a string argument, refusing a missing or non-string one.
fn arg_string(ctx: *mut JSContext, value: Option<&JSValue>) -> Result<String, String> {
    let value = value
        .copied()
        .ok_or("a callback name argument is required")?;
    let mut len = 0;
    let ptr = unsafe { JS_ToCStringLen2(ctx, &mut len, value, false) };
    if ptr.is_null() {
        return Err("the argument is not a string".to_string());
    }
    let text = cstr_to_rust(ptr).unwrap_or_default().to_string();
    unsafe { JS_FreeCString(ctx, ptr) };
    Ok(text)
}

/// Reads and clears a pending exception, so a failed call does not leave the context dirty.
fn take_exception(ctx: *mut JSContext) -> Option<String> {
    unsafe {
        let exception = JS_GetException(ctx);
        let mut len = 0;
        let ptr = JS_ToCStringLen2(ctx, &mut len, exception, false);
        let message = if ptr.is_null() {
            None
        } else {
            let text = cstr_to_rust(ptr).map(str::to_string);
            JS_FreeCString(ctx, ptr);
            text
        };
        JS_FreeValue(ctx, exception);
        message
    }
}

/// Calls a global function with no arguments, the way `call_function` does.
fn call_global_function(ctx: *mut JSContext, name: &str) -> Result<(), String> {
    unsafe {
        let global = JS_GetGlobalObject(ctx);
        let cname = CString::new(name).map_err(|e| e.to_string())?;
        let function = JS_GetPropertyStr(ctx, global, cname.as_ptr());

        let result = JS_Call(ctx, function, global, 0, std::ptr::null_mut());
        JS_FreeValue(ctx, function);
        JS_FreeValue(ctx, global);

        if JS_HasException(ctx) {
            let message = take_exception(ctx).unwrap_or_else(|| "unknown error".to_string());
            return Err(format!("{name} failed: {message}"));
        }
        JS_FreeValue(ctx, result);
        Ok(())
    }
}

pub struct JSBridge {
    rt: Arc<Mutex<*mut JSRuntime>>,
    ctx: Arc<Mutex<*mut JSContext>>,
    timers: Arc<Mutex<Timers>>,
}

// QuickJS is single threaded and its pointers are neither `Send` nor `Sync`, so these `Arc`s
// only share the engine between owners on one thread; the C ABI keeps that contract.
#[allow(clippy::arc_with_non_send_sync)]
impl JSBridge {
    pub fn new() -> Result<Self, String> {
        unsafe {
            let rt = JS_NewRuntime();
            if rt.is_null() {
                return Err("the QuickJS runtime could not be created".to_string());
            }

            let ctx = JS_NewContext(rt);
            if ctx.is_null() {
                libquickjs_ng_sys::JS_FreeRuntime(rt);
                return Err("the QuickJS context could not be created".to_string());
            }

            let bridge = JSBridge {
                rt: Arc::new(Mutex::new(rt)),
                ctx: Arc::new(Mutex::new(ctx)),
                timers: Arc::new(Mutex::new(Timers::new())),
            };
            bridge.init_timer_api()?;
            Ok(bridge)
        }
    }

    /// Registers the same timer functions the Lua bridge exposes, so a script behaves the same in
    /// either runtime.
    fn init_timer_api(&self) -> Result<(), String> {
        let timers_add = Arc::clone(&self.timers);
        self.export_function("addTimer", move |ctx, args| {
            let delay = arg_f64(ctx, args.first())?;
            let callback = arg_string(ctx, args.get(1))?;
            let handle = lock_timers(&timers_add)?.add(delay, &callback);
            Ok(js_int(handle))
        })?;

        let timers_poll = Arc::clone(&self.timers);
        self.export_function("pollTimers", move |ctx, _args| {
            // The lock is released before the callbacks run, so one of them may schedule another
            // timer or remove one.
            let due = lock_timers(&timers_poll)?.poll();
            for func_name in due {
                call_global_function(ctx, &func_name)?;
            }
            Ok(js_undefined())
        })?;

        let timers_remove = Arc::clone(&self.timers);
        self.export_function("removeTimer", move |ctx, args| {
            let handle = arg_i32(ctx, args.first())?;
            lock_timers(&timers_remove)?.remove(handle);
            Ok(js_undefined())
        })?;

        Ok(())
    }

    pub fn load_script_file(&self, path: &str, is_module: bool) -> Result<(), String> {
        let content = fs::read_to_string(Path::new(path))
            .map_err(|e| format!("Failed to read file: {}", e))?;
        self.load_script_content(&content, is_module)
    }

    pub fn load_script_content(&self, script: &str, is_module: bool) -> Result<(), String> {
        unsafe {
            let ctx = self.ctx.lock().unwrap();
            let cscript = CString::new(script).unwrap();
            let filename = CString::new("script.js").unwrap();

            let eval_flags = if is_module {
                libquickjs_ng_sys::JS_EVAL_TYPE_MODULE as i32
            } else {
                libquickjs_ng_sys::JS_EVAL_TYPE_GLOBAL as i32
            };

            let val = JS_Eval(
                *ctx,
                cscript.as_ptr(),
                script.len(),
                filename.as_ptr(),
                eval_flags,
            );

            self.eval_and_handle_errors(*ctx, val)
        }
    }

    pub fn load_bytecode_file(&self, path: &str) -> Result<(), String> {
        let bytecode = fs::read(Path::new(path))
            .map_err(|e| format!("Failed to read bytecode file: {}", e))?;
        self.load_bytecode_content(&bytecode)
    }

    pub fn load_bytecode_content(&self, bytecode: &[u8]) -> Result<(), String> {
        unsafe {
            let ctx = self.ctx.lock().unwrap();

            let obj = libquickjs_ng_sys::JS_ReadObject(
                *ctx,
                bytecode.as_ptr(),
                bytecode.len(),
                libquickjs_ng_sys::JS_READ_OBJ_BYTECODE as i32,
            );

            if let Err(e) = self.eval_and_handle_errors(*ctx, obj) {
                return Err(e.replace("Execution", "Bytecode read"));
            }

            let val = libquickjs_ng_sys::JS_EvalFunction(*ctx, obj);
            self.eval_and_handle_errors(*ctx, val)
                .map_err(|e| e.replace("Execution", "Bytecode evaluation"))
        }
    }

    pub fn call_function(&self, func_name: &str, arg: &str) -> Result<String, String> {
        unsafe {
            let ctx = self.ctx.lock().unwrap();
            let global = JS_GetGlobalObject(*ctx);

            let cname = CString::new(func_name).unwrap();
            let func_val = JS_GetPropertyStr(*ctx, global, cname.as_ptr());

            if JS_HasException(*ctx) {
                JS_FreeValue(*ctx, global);
                return Err(format!("Function {} not found", func_name));
            }

            let arg_val =
                JS_NewStringLen(*ctx, arg.as_ptr() as *const i8, arg.len() as libc::size_t);

            let result = JS_Call(
                *ctx,
                func_val,
                global,
                1,
                &arg_val as *const JSValue as *mut JSValue,
            );

            JS_FreeValue(*ctx, func_val);
            JS_FreeValue(*ctx, global);

            if JS_HasException(*ctx) {
                let message = take_exception(*ctx).unwrap_or_else(|| "unknown error".to_string());
                return Err(format!("Function call error: {message}"));
            }

            let mut len = 0;
            let ptr = libquickjs_ng_sys::JS_ToCStringLen2(*ctx, &mut len, result, false);
            let result_str = cstr_to_rust(ptr).unwrap_or("").to_string();

            if !ptr.is_null() {
                libquickjs_ng_sys::JS_FreeCString(*ctx, ptr);
            }
            JS_FreeValue(*ctx, result);

            Ok(result_str)
        }
    }

    pub fn export_function<F>(&self, name: &str, func: F) -> Result<(), String>
    where
        F: Fn(*mut JSContext, Vec<JSValue>) -> Result<JSValue, String> + Send + Sync + 'static,
    {
        unsafe {
            let ctx = self.ctx.lock().unwrap();
            let global = JS_GetGlobalObject(*ctx);
            let cname = CString::new(name).map_err(|e| e.to_string())?;

            // Every registered closure gets an id that travels in the function object's `magic`,
            // which is how the trampoline finds it again when the script calls the function.
            let magic = NEXT_CALLBACK_ID.fetch_add(1, Ordering::SeqCst);
            js_callbacks().insert(magic, Arc::new(func));

            // `JS_NewCFunction2` declares the generic callback type, while the magic variant takes
            // one more parameter; QuickJS casts the same way in its own `JS_NewCFunctionMagic`.
            let callback: JSCFunction =
                std::mem::transmute::<JSCFunctionMagic, JSCFunction>(Some(js_callback_trampoline));

            let js_func = JS_NewCFunction2(
                *ctx,
                callback,
                cname.as_ptr(),
                0,
                JSCFunctionEnum_JS_CFUNC_generic_magic,
                magic,
            );

            JS_SetPropertyStr(*ctx, global, cname.as_ptr(), js_func);
            JS_FreeValue(*ctx, global);
            Ok(())
        }
    }

    unsafe fn eval_and_handle_errors(
        &self,
        ctx: *mut JSContext,
        value: JSValue,
    ) -> Result<(), String> {
        // Wrap all FFI calls in unsafe blocks
        unsafe {
            if libquickjs_ng_sys::JS_HasException(ctx) {
                let exception = libquickjs_ng_sys::JS_GetException(ctx);
                let string_val = libquickjs_ng_sys::JS_ToString(ctx, exception);

                let mut len = 0;
                let ptr = libquickjs_ng_sys::JS_ToCStringLen2(ctx, &mut len, string_val, false);
                let err_msg = cstr_to_rust(ptr).unwrap_or("Unknown error").to_string();

                if !ptr.is_null() {
                    libquickjs_ng_sys::JS_FreeCString(ctx, ptr);
                }
                libquickjs_ng_sys::JS_FreeValue(ctx, exception);
                libquickjs_ng_sys::JS_FreeValue(ctx, string_val);
                return Err(format!("Execution error: {}", err_msg));
            }

            libquickjs_ng_sys::JS_FreeValue(ctx, value);
        }
        Ok(())
    }
}

impl Drop for JSBridge {
    fn drop(&mut self) {
        unsafe {
            let ctx = self.ctx.lock().unwrap();
            let rt = self.rt.lock().unwrap();
            libquickjs_ng_sys::JS_FreeContext(*ctx);
            libquickjs_ng_sys::JS_FreeRuntime(*rt);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `call_function` expects the script function to return a string, so the timers are polled
    /// through these wrappers.
    const TIMERS: &str = r#"
        let fired = 0;

        function onFire() {
            fired += 1;
            if (fired === 1) {
                // A callback is allowed to schedule another timer while polling.
                addTimer(0, "onFire");
            }
        }

        const timer = addTimer(0, "onFire");

        function count() { return String(fired); }
        function poll() { pollTimers(); return String(fired); }
        function cancel() { removeTimer(timer); return "ok"; }
    "#;

    #[test]
    fn a_timer_runs_once_and_a_callback_can_schedule_another() {
        let bridge = JSBridge::new().unwrap();
        bridge.load_script_content(TIMERS, false).unwrap();

        // Registering a timer does not run it.
        assert_eq!(bridge.call_function("count", "").unwrap(), "0");
        assert_eq!(bridge.call_function("poll", "").unwrap(), "1");

        // The timer the callback scheduled above now runs.
        assert_eq!(bridge.call_function("poll", "").unwrap(), "2");

        // Nothing is left to run, and cancelling a timer that already ran is harmless.
        assert_eq!(bridge.call_function("poll", "").unwrap(), "2");
        assert_eq!(bridge.call_function("cancel", "").unwrap(), "ok");
        assert_eq!(bridge.call_function("poll", "").unwrap(), "2");
    }

    #[test]
    fn a_removed_timer_never_runs() {
        let bridge = JSBridge::new().unwrap();
        bridge
            .load_script_content(
                r#"
                let fired = 0;

                function onFire() { fired += 1; }

                const dropped = addTimer(0, "onFire");

                function drop() { removeTimer(dropped); return "ok"; }
                function poll() { pollTimers(); return String(fired); }
                "#,
                false,
            )
            .unwrap();

        assert_eq!(bridge.call_function("drop", "").unwrap(), "ok");
        assert_eq!(bridge.call_function("poll", "").unwrap(), "0");
    }

    #[test]
    fn a_missing_callback_is_reported() {
        let bridge = JSBridge::new().unwrap();
        bridge
            .load_script_content(
                r#"
                addTimer(0, "notAFunction");

                function poll() { pollTimers(); return "ok"; }
                "#,
                false,
            )
            .unwrap();

        let error = bridge.call_function("poll", "").unwrap_err();
        assert!(error.contains("notAFunction"), "unexpected error: {error}");
    }

    #[test]
    fn every_registered_function_keeps_its_own_behaviour() {
        let bridge = JSBridge::new().unwrap();
        bridge
            .export_function("double", |ctx, args| {
                Ok(js_int(arg_i32(ctx, args.first())? * 2))
            })
            .unwrap();
        bridge
            .export_function("triple", |ctx, args| {
                Ok(js_int(arg_i32(ctx, args.first())? * 3))
            })
            .unwrap();
        bridge
            .load_script_content(
                r#"
                function doubled() { return String(double(21)); }
                function tripled() { return String(triple(21)); }
                "#,
                false,
            )
            .unwrap();

        // The function registered first must still reach its own closure.
        assert_eq!(bridge.call_function("doubled", "").unwrap(), "42");
        assert_eq!(bridge.call_function("tripled", "").unwrap(), "63");
    }

    #[test]
    fn a_failing_callback_reaches_the_script_as_a_thrown_error() {
        let bridge = JSBridge::new().unwrap();
        bridge
            .export_function("explode", |_ctx, _args| Err("boom".to_string()))
            .unwrap();
        bridge
            .load_script_content(
                r#"
                function tryIt() {
                    try {
                        explode();
                        return "no error";
                    } catch (e) {
                        return String(e);
                    }
                }
                "#,
                false,
            )
            .unwrap();

        assert_eq!(bridge.call_function("tryIt", "").unwrap(), "boom");
    }
}
