use libquickjs_ng_sys::{
    JS_Eval, JS_FreeContext, JS_FreeRuntime, JS_NewContext, JS_NewRuntime, JS_WriteObject,
};
use std::ffi::CString;
use std::fs::{File, read_to_string};
use std::io::Write;
use std::path::Path;
use std::process;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: qjsc [--module] <input.js> <output.qbc>");
        process::exit(1);
    }

    let is_module = args.iter().any(|arg| arg == "--module");
    let input_path = Path::new(&args[args.len() - 2]);
    let output_path = Path::new(&args[args.len() - 1]);

    unsafe {
        let rt = JS_NewRuntime();
        let ctx = JS_NewContext(rt);

        let script = match read_to_string(input_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Failed to read {}: {}", input_path.display(), e);
                process::exit(1);
            }
        };

        let script_len = script.len();
        let cscript = match CString::new(script) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Invalid script content: {}", e);
                process::exit(1);
            }
        };

        let cfilename = match CString::new(input_path.to_string_lossy().into_owned()) {
            Ok(s) => s.into_raw(),
            Err(_) => {
                eprintln!("Invalid filename");
                process::exit(1);
            }
        };

        let obj = JS_Eval(
            ctx,
            cscript.as_ptr(),
            script_len,
            cfilename,
            libquickjs_ng_sys::JS_EVAL_FLAG_COMPILE_ONLY as i32
                | if is_module {
                    libquickjs_ng_sys::JS_EVAL_TYPE_MODULE as i32
                } else {
                    libquickjs_ng_sys::JS_EVAL_TYPE_GLOBAL as i32
                },
        );

        let mut out_file = match File::create(output_path) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("Failed to create {}: {}", output_path.display(), e);
                process::exit(1);
            }
        };

        let mut buf_len = 0;
        let _buf_ptr: *mut u8 = std::ptr::null_mut();
        JS_WriteObject(ctx, &mut buf_len as *mut usize, obj, 0);

        // Allocate buffer
        let mut buf = vec![0u8; buf_len];
        JS_WriteObject(ctx, buf.as_mut_ptr() as *mut usize, obj, 0);

        if let Err(e) = out_file.write_all(&buf) {
            eprintln!("Failed to write bytecode: {}", e);
            process::exit(1);
        }

        // Cleanup
        JS_FreeContext(ctx);
        JS_FreeRuntime(rt);
        let _ = CString::from_raw(cfilename);
    }
}
