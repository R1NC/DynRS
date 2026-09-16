pub mod core {
    pub mod crypto;
    pub mod db;
    pub mod kv;
    pub mod lua;
    pub mod net;
    pub mod qjs;
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
    pub mod net;
    pub mod qjs;
    pub mod util;
    pub mod zip;
}
