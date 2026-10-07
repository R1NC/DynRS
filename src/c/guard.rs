//! Where the C ABI's panic discipline is documented.
//!
//! The machinery lives in two places, both at the crate root because that is where a
//! `#[macro_export]` macro has to sit for the `c` submodules to see it:
//!
//! * [`crate::ffi_return!`] wraps an entry point's body so a panic becomes a failed call.
//! * [`crate::FfiPanicValue`] is the "nothing worked" value each return type hands back.
//!
//! This module exists so that the reasoning behind that arrangement has a home next to the code
//! it governs, and so `src/lib.rs` does not have to carry the whole explanation.
//!
//! # Why the boundary needs this at all
//!
//! Every `pub extern "C" fn` in this module tree is reached from a foreign host: a JNI or NAPI
//! wrapper, or a C/C++ desktop program. A panic that escapes one of them cannot be handled by that
//! host. Since Rust 1.81 the runtime aborts with `panic in a function that cannot unwind`, and
//! before it gets there the unwinder has already run part of the way down the stack, so the
//! process dies with resource destructors half-executed. Neither outcome is usable, and the
//! default unwind strategy alone does not prevent it.
//!
//! `panic = "abort"` is the other way to make the boundary defined, and it is deliberately not
//! used: it would abort on the *first* panic, so the host would still lose the process and get no
//! information about the failure. Catching inside the boundary means the caller sees the same
//! value it already handles for a rejected argument.
//!
//! # Coverage
//!
//! Every one of the 67 `pub extern "C" fn` in this module tree runs its body inside the macro, the
//! deallocations included: they cannot panic today, but a `ffi_return!` there costs nothing and
//! keeps the rule "an entry point is guarded" free of exceptions to remember. [`crate::ffi_panic_hook`]
//! at the crate root is the sixty-eighth guarded function, and the only one that is reached through
//! a function pointer rather than by name. The private helpers are guarded through their callers
//! rather than themselves, so the catch sits on the boundary and not one frame inside it — except
//! `_ngenrs_qjs_load` and `_ngenrs_z_process`, which four and two entry points share respectively
//! and which therefore carry their own.
//!
//! One function is guarded in a different way on purpose: `js_callback_trampoline` in
//! [`crate::core::qjs`] is called *by* QuickJS rather than by the host, so its catch converts the
//! panic into a thrown JS exception — the same shape as a callback that returned `Err` — instead of
//! a return value the caller could not receive.
//!
//! # What a guarded entry point reports
//!
//! Every entry point that can fail returns a [`crate::DynrsStatus`], and hands its result back
//! through out-parameters. The status is what tells the caller apart the answers that used to be one
//! value:
//!
//! * [`crate::DynrsStatus::Ok`] — the call did what it says.
//! * [`crate::DynrsStatus::Empty`] — there was nothing to report, and that is not an error: no value
//!   under this key, no row is current, the response carries no body. It is kept separate from `Ok`
//!   so that a caller cannot read a zero as data.
//! * [`crate::DynrsStatus::InvalidArgument`] — the caller's input was rejected: a null string, text
//!   that is not UTF-8, an empty key, a column the query did not select, a port that does not fit.
//! * [`crate::DynrsStatus::InvalidHandle`] — the handle is null, or cannot answer in its current
//!   state (a result set before `ngenrs_db_next_row`).
//! * [`crate::DynrsStatus::Failed`] — the operation itself failed: the network, the store, the
//!   engine.
//!
//! [`crate::DynrsStatus::Panicked`] is what the macro returns when the body panicked instead, so a
//! host can tell "the library broke" from "my call was wrong".
//!
//! Data crosses as a pointer and a length rather than as a C string, because a value the caller or
//! the remote produced can contain a NUL. Text the library generates itself — error messages, PEM,
//! header names — is a C string, since it cannot contain one. `README.md` has the ownership table.
