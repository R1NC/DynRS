/// The status every data-returning C ABI entry point reports.
///
/// Errors travel in the return value rather than in a nullable out-pointer, so that "the call was
/// rejected", "there is no such value" and "the value is empty" are three different answers instead
/// of three readings of one null. The numeric values are part of the ABI and must not be renumbered.
#[repr(i32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DynrsStatus {
    /// The call ran. The out-parameters hold the result, which may be empty.
    Ok = 0,
    /// The call ran and there is no value here: no such column, no such key, a response with no
    /// body. Distinct from an error, and distinct from an empty value.
    Empty = 1,
    /// An argument was refused: a null pointer where one is required, a key length that is not
    /// 16/24/32 bytes, a nonce that is not 12 bytes, an unknown format.
    InvalidArgument = 2,
    /// The handle is null or is not a handle this library produced.
    InvalidHandle = 3,
    /// The operation was understood and failed on its own terms: a GCM tag that does not verify, a
    /// database that will not open, a request that did not go through.
    Failed = 4,
    /// The body panicked. The message reached the hook installed by [`ffi_panic_hook`].
    Panicked = 5,
}

impl FfiPanicValue for DynrsStatus {
    const FFI_PANIC_VALUE: Self = DynrsStatus::Panicked;
}

/// The value a guarded C ABI entry point hands back when its body panicked.
///
/// Every C ABI return type here is a plain scalar or a pointer, so the "nothing worked" value is
/// the type's zero. `bool` and `f64` need their own implementations because Rust has no `Zero` for
/// them. It is an associated const rather than a method so that `ffi_return!` can name it through
/// an inferred type.
pub trait FfiPanicValue {
    const FFI_PANIC_VALUE: Self;
}

impl FfiPanicValue for bool {
    const FFI_PANIC_VALUE: Self = false;
}

impl FfiPanicValue for f64 {
    const FFI_PANIC_VALUE: Self = 0.0;
}

/// A `void` entry point: the caller has nothing to distinguish, so the guard only has to stop the
/// panic from escaping.
impl FfiPanicValue for () {
    const FFI_PANIC_VALUE: Self = ();
}

impl<T> FfiPanicValue for *mut T {
    const FFI_PANIC_VALUE: Self = std::ptr::null_mut();
}

impl<T> FfiPanicValue for *const T {
    const FFI_PANIC_VALUE: Self = std::ptr::null();
}

macro_rules! ffi_panic_value_for_int {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl FfiPanicValue for $ty {
                const FFI_PANIC_VALUE: Self = 0;
            }
        )+
    };
}

ffi_panic_value_for_int!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

/// Runs a C ABI body inside `catch_unwind`, returning [`FfiPanicValue::FFI_PANIC_VALUE`] if it
/// panicked so that no panic ever unwinds out of an `extern "C"` function.
///
/// The library ships as a `staticlib` that a platform wrapper (JNI, NAPI, a C/C++ host) embeds, so
/// an escaping panic is not something the host can handle: since Rust 1.81 it aborts with
/// `panic in a function that cannot unwind`, after the unwinder has already run part of the way
/// down the stack. Catching it here keeps a bug in Rust a failed call instead of a dead process.
///
/// The body has to end in an expression of the enclosing function's return type. `return` keeps
/// working inside it, and the closure moves its captures, so `&mut` locals are accepted without
/// the `UnwindSafe` complaint.
///
/// ```ignore
/// #[unsafe(no_mangle)]
/// pub extern "C" fn ngenrs_thing(handle: *mut c_void) -> bool {
///     ffi_return! {
///         let Some(handle) = (unsafe { handle.as_ref() }) else {
///             return false;
///         };
///         handle.run().is_ok()
///     }
/// }
/// ```
///
/// Every entry point in `c` uses this, and so does `ffi_panic_hook` itself. One function is guarded
/// differently on purpose: `js_callback_trampoline` in `core::qjs` is called *by* QuickJS rather
/// than by the host, so its catch turns the panic into a thrown JS exception instead of a return
/// value.
#[macro_export]
macro_rules! ffi_return {
    ($($body:tt)*) => {
        match ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(
            move || -> _ { $($body)* },
        )) {
            ::std::result::Result::Ok(value) => value,
            ::std::result::Result::Err(payload) => {
                $crate::report_panic(&payload);
                <_ as $crate::FfiPanicValue>::FFI_PANIC_VALUE
            }
        }
    };
}

/// A message about something that went wrong inside this library, delivered to whoever registered
/// [`ffi_panic_hook`].
pub type PanicHook = extern "C" fn(*const std::os::raw::c_char);

/// Registers the callback that receives panic messages, replacing any previous one. Without a
/// registration the messages go to stderr.
///
/// A panic caught at the C boundary is turned into [`DynrsStatus::Panicked`], but the caller cannot
/// be told *why* through a return value, and a host that never sees the reason cannot report it.
/// This is where the reason goes: a panic hook is the one part of Rust's panic handling that runs
/// before the unwinder starts, so the message is still intact here.
#[unsafe(no_mangle)]
pub extern "C" fn ffi_panic_hook(hook: Option<PanicHook>) {
    ffi_return! {
        *PANIC_HOOK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = hook;
    }
}

/// A `Mutex` rather than a `OnceLock`: registering a hook after one is already installed has to
/// replace it, and a hook installed at startup is the point.
static PANIC_HOOK: std::sync::Mutex<Option<PanicHook>> = std::sync::Mutex::new(None);

/// Turns a panic payload into text. `Box<dyn Any>` holds whatever the panicking site passed, which
/// is almost always `&str` or `String`.
pub fn panic_message(payload: &(dyn std::any::Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("the panic carried no message")
}

/// Reports a caught panic through [`ffi_panic_hook`], or to stderr when no hook is registered.
///
/// This runs *after* `catch_unwind` has returned, so an unwinding panic here would leave the
/// `extern "C"` function and abort the host. Every step is therefore panic-free: the hook call is
/// itself caught, and a message that cannot become a C string still reaches stderr.
pub fn report_panic(payload: &(dyn std::any::Any + Send)) {
    let message = panic_message(payload);
    let hook = *PANIC_HOOK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    // A message with an interior NUL cannot be a C string; the hook gets what it can read, and the
    // rest is not lost because stderr still sees the whole thing.
    let as_c_string = std::ffi::CString::new(message).ok();
    if as_c_string.is_none() {
        eprintln!("dynrs: a call panicked: {message}");
    }

    if let Some(hook) = hook {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match as_c_string {
            Some(text) => hook(text.as_ptr()),
            None => hook(std::ptr::null()),
        }));
    } else if as_c_string.is_some() {
        eprintln!("dynrs: a call panicked: {message}");
    }
}

pub mod core {
    pub mod crypto;
    pub mod db;
    pub mod kv;
    pub mod lua;
    // wasm has no socket stack, so the network module is not part of that target. DynXX leaves
    // curl out of its Emscripten build for the same reason.
    #[cfg(not(target_arch = "wasm32"))]
    pub mod net;
    pub mod qjs;
    // The two test suites that speak HTTP share a server on `127.0.0.1`.
    #[cfg(test)]
    pub(crate) mod net_test_server;
    pub mod timer;
    pub mod zip;
}

// Every item here is a C ABI boundary that receives raw pointers from foreign code, so the
// pointer contract cannot be expressed in the type system. Clippy's suggestion to mark them
// `unsafe fn` would not change the ABI, only make the Rust-side callers wrap every call.
//
// Because a panic may not unwind out of one of these functions, every entry point runs its body
// inside `guard::ffi_return!`; see that module for why the default unwind strategy is not enough.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub mod c {
    pub mod crypto;
    pub mod db;
    pub mod guard;
    pub mod kv;
    pub mod lua;
    #[cfg(not(target_arch = "wasm32"))]
    pub mod net;
    pub mod qjs;
    pub mod util;
    pub mod zip;
}
