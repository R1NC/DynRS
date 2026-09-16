use crate::core::timer::{Timers, lock_timers};
use mlua::{FromLuaMulti, Function, Lua, UserData};
use std::path::Path;
use std::result::Result;
use std::sync::{Arc, Mutex};

/// The handle `addTimer` hands back to a script.
#[derive(Clone)]
struct TimerHandle(i32);

impl UserData for TimerHandle {}

pub struct LuaBridge {
    lua: Lua,
    timers: Arc<Mutex<Timers>>,
}

impl LuaBridge {
    pub fn new() -> Result<Self, String> {
        let bridge = LuaBridge {
            lua: Lua::new(),
            timers: Arc::new(Mutex::new(Timers::new())),
        };
        bridge.init_timer_api()?;
        Ok(bridge)
    }

    /// Registers the timer functions the JS bridge exposes as well, so a script behaves the same in
    /// either runtime.
    fn init_timer_api(&self) -> Result<(), String> {
        let timers_add = Arc::clone(&self.timers);
        self.export_function("addTimer", move |lua, args: mlua::MultiValue| {
            let (delay, callback) = <(f64, String)>::from_lua_multi(args, lua)?;

            let handle = lock_timers(&timers_add)
                .map_err(mlua::Error::RuntimeError)?
                .add(delay, &callback);
            Ok(TimerHandle(handle))
        })
        .map_err(|e| e.to_string())?;

        let timers_poll = Arc::clone(&self.timers);
        self.export_function("pollTimers", move |lua, _args: mlua::MultiValue| {
            // The lock is released before the callbacks run, so one of them may schedule another
            // timer or remove one.
            let due = lock_timers(&timers_poll)
                .map_err(mlua::Error::RuntimeError)?
                .poll();
            for func_name in due {
                let func: mlua::Value = lua.globals().get(&*func_name)?;
                let func = match func {
                    mlua::Value::Function(func) => func,
                    _ => {
                        return Err(mlua::Error::RuntimeError(format!(
                            "the timer callback {func_name} is not defined"
                        )));
                    }
                };
                func.call::<_, ()>(())?;
            }
            Ok(())
        })
        .map_err(|e| e.to_string())?;

        let timers_remove = Arc::clone(&self.timers);
        self.export_function("removeTimer", move |lua, args: mlua::MultiValue| {
            let (userdata,) = <(mlua::AnyUserData,)>::from_lua_multi(args, lua)?;
            let handle = userdata.borrow::<TimerHandle>()?.clone();
            lock_timers(&timers_remove)
                .map_err(mlua::Error::RuntimeError)?
                .remove(handle.0);
            Ok(())
        })
        .map_err(|e| e.to_string())?;

        Ok(())
    }

    pub fn load_file(&self, path: &str) -> Result<(), String> {
        let path = Path::new(path);
        self.lua.load(path).exec().map_err(|e| e.to_string())
    }

    pub fn load_string(&self, script: &str) -> Result<(), String> {
        self.lua.load(script).exec().map_err(|e| e.to_string())
    }

    pub fn call_function(&self, func_name: &str, arg: &str) -> Result<String, String> {
        let func: Function = self
            .lua
            .globals()
            .get(func_name)
            .map_err(|e| e.to_string())?;
        func.call::<_, String>(arg).map_err(|e| e.to_string())
    }

    /// Registers a Rust function as a global the script can call. The closure receives all of the
    /// script's arguments.
    pub fn export_function<F, R>(&self, name: &str, func: F) -> Result<(), String>
    where
        F: Fn(&Lua, mlua::MultiValue) -> mlua::Result<R> + 'static,
        R: for<'lua> mlua::IntoLuaMulti<'lua>,
    {
        let lua_func = self.lua.create_function(func).map_err(|e| e.to_string())?;
        self.lua
            .globals()
            .set(name, lua_func)
            .map_err(|e| e.to_string())
    }

    // Generic version that works with any Rust function
    pub fn export_rust_fn<F, A, R>(&self, name: &str, func: F) -> Result<(), String>
    where
        F: Fn(A) -> R + 'static,
        A: for<'lua> mlua::FromLuaMulti<'lua>,
        R: for<'lua> mlua::IntoLuaMulti<'lua>,
    {
        let lua_func = self
            .lua
            .create_function(move |_, args| Ok(func(args)))
            .map_err(|e| e.to_string())?;
        self.lua
            .globals()
            .set(name, lua_func)
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `call_function` expects the script function to return a string, so the timers are polled
    /// through these wrappers.
    const TIMERS: &str = r#"
        local fired = 0

        function on_fire()
            fired = fired + 1
            if fired == 1 then
                -- A callback is allowed to schedule another timer while polling.
                addTimer(0, "on_fire")
            end
        end

        local timer = addTimer(0, "on_fire")

        function count() return tostring(fired) end
        function poll() pollTimers() return tostring(fired) end
        function cancel() removeTimer(timer) return "ok" end
    "#;

    #[test]
    fn a_timer_runs_once_and_a_callback_can_schedule_another() {
        let bridge = LuaBridge::new().unwrap();
        bridge.load_string(TIMERS).unwrap();

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
        let bridge = LuaBridge::new().unwrap();
        bridge
            .load_string(
                r#"
                local fired = 0

                function on_fire() fired = fired + 1 end

                local dropped = addTimer(0, "on_fire")

                function drop() removeTimer(dropped) return "ok" end
                function poll() pollTimers() return tostring(fired) end
                "#,
            )
            .unwrap();

        assert_eq!(bridge.call_function("drop", "").unwrap(), "ok");
        assert_eq!(bridge.call_function("poll", "").unwrap(), "0");
    }

    #[test]
    fn a_missing_callback_is_reported() {
        let bridge = LuaBridge::new().unwrap();
        bridge
            .load_string(
                r#"
                addTimer(0, "not_a_function")

                function poll() pollTimers() return "ok" end
                "#,
            )
            .unwrap();

        let error = bridge.call_function("poll", "").unwrap_err();
        assert!(
            error.contains("not_a_function"),
            "unexpected error: {error}"
        );
    }
}
