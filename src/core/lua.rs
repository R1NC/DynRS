use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use mlua::{Lua, Function, UserData, FromLua};
use std::path::Path;
use std::result::Result;

#[derive(Clone)]
struct TimerHandle(usize);

struct TimerEntry {
    end_time: Instant,
    callback: String,  // Store function name instead of Function
}

impl UserData for TimerHandle {}

struct TimerState {
    next_id: usize,
    active_timers: HashMap<usize, TimerEntry>,
}

pub struct LuaBridge {
    lua: Lua,
    timers: Arc<Mutex<TimerState>>,
}

impl LuaBridge {
    pub fn new() -> Result<Self, String> {
        let lua = Lua::new();
        let timers = Arc::new(Mutex::new(TimerState {
            next_id: 1,
            active_timers: HashMap::new(),
        }));

        let bridge = LuaBridge { lua, timers };
        bridge.init_timer_api()?;
        Ok(bridge)
    }

    fn init_timer_api(&self) -> Result<(), String> {
        let timers_add = self.timers.clone();
        
        self.export_function("addTimer", move |lua, value: mlua::Value| {
            let table = mlua::Table::from_lua(value, lua)?;
            let delay: f64 = table.get(1)?;
            let callback_name: String = table.get(2)?;
            
            let handle = {
                let mut state = timers_add.lock().unwrap();
                let id = state.next_id;
                state.next_id += 1;
                state.active_timers.insert(id, TimerEntry {
                    end_time: Instant::now() + Duration::from_secs_f64(delay),
                    callback: callback_name,  // Store function name
                });
                TimerHandle(id)
            };
            Ok(handle)
        }).map_err(|e| e.to_string())?;

        let timers_poll = self.timers.clone();
        self.export_function("pollTimers", move |lua, _: mlua::Value| {
            let mut state = timers_poll.lock().unwrap();
            let now = Instant::now();
            let mut expired = Vec::new();
            
            state.active_timers.retain(|id, entry| {
                if entry.end_time <= now {
                    expired.push((*id, entry.callback.clone()));
                    false
                } else {
                    true
                }
            });
            
            // Look up and call functions by name
            for (_, func_name) in expired {
                let func: Function = lua.globals().get(&*func_name)?;  // Added dereference here
                func.call::<_, ()>(())?;
            }
            Ok(())
        }).map_err(|e| e.to_string())?;
    
        let timers_remove = self.timers.clone();
        self.export_function("removeTimer", move |lua, value: mlua::Value| {
            let handle = TimerHandle::from_lua(value, lua)?;
            let mut state = timers_remove.lock().unwrap();
            state.active_timers.remove(&handle.0);
            Ok(())
        }).map_err(|e| e.to_string())?;
    
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
        let func: Function = self.lua.globals().get(func_name).map_err(|e| e.to_string())?;
        func.call::<_, String>(arg).map_err(|e| e.to_string())
    }

    pub fn export_function<'a, F, R>(&self, name: &str, func: F) -> Result<(), String>
    where
        F: Fn(&Lua, mlua::Value) -> mlua::Result<R> + 'static,
        R: for<'lua> mlua::ToLuaMulti<'lua>,
    {
        let lua_func = self.lua.create_function(func).map_err(|e| e.to_string())?;
        self.lua.globals().set(name, lua_func).map_err(|e| e.to_string())
    }

    // Generic version that works with any Rust function
    pub fn export_rust_fn<F, A, R>(&self, name: &str, func: F) -> Result<(), String>
    where
        F: Fn(A) -> R + 'static,
        A: for<'lua> mlua::FromLuaMulti<'lua>,
        R: for<'lua> mlua::ToLuaMulti<'lua>,
    {
        let lua_func = self.lua.create_function(move |_, args| Ok(func(args))).map_err(|e| e.to_string())?;
        self.lua.globals().set(name, lua_func).map_err(|e| e.to_string())
    }
}