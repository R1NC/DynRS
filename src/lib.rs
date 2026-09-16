pub mod core {
    pub mod crypto;
    pub mod db;
    pub mod kv;
    pub mod lua;
    // DynXX leaves curl out of its Emscripten build, so the wasm targets leave the network out
    // too; every other module builds there.
    #[cfg(not(target_arch = "wasm32"))]
    pub mod net;
    pub mod qjs;
    pub mod timer;
    pub mod zip;
}

// Every item here is a C ABI boundary that receives raw pointers from foreign code, so the
// pointer contract cannot be expressed in the type system. Clippy's suggestion to mark them
// `unsafe fn` would not change the ABI, only make the Rust-side callers wrap every call.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub mod c {
    pub mod crypto;
    pub mod db;
    pub mod kv;
    pub mod lua;
    #[cfg(not(target_arch = "wasm32"))]
    pub mod net;
    pub mod qjs;
    pub mod util;
    pub mod zip;
}
