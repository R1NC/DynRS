use std::ffi::CString;
use std::os::raw::c_int;
use std::io;
use crate::core::zip::{CompressionFormat, compress, decompress};
use crate::c::util::{cbytes_to_rust, rust_to_cbytes};

fn _ngenrs_z_process(
    format: c_int,
    input: *const u8,
    input_len: usize,
    output: *mut *mut u8,
    output_len: *mut usize,
    operation: fn(std::io::Cursor<&'static [u8]>, CompressionFormat) -> io::Result<Vec<u8>>,
) -> *mut u8 {
    let format = match format {
        0 => CompressionFormat::Gzip,
        1 => CompressionFormat::Zlib,
        2 => CompressionFormat::Raw,
        _ => return std::ptr::null_mut(),
    };

    if input.is_null() || output.is_null() || output_len.is_null() {
        return std::ptr::null_mut();
    }

    let input_slice = match cbytes_to_rust(input, input_len) {
        Some(slice) => slice,
        None => return CString::new("Invalid input buffer").unwrap().into_raw() as *mut u8,
    };

    match operation(std::io::Cursor::new(input_slice), format) {
        Ok(result) => {
            let (ptr, len) = rust_to_cbytes(result);
            unsafe {
                *output = ptr;
                *output_len = len;
            }
            std::ptr::null_mut()
        }
        Err(e) => CString::new(e.to_string()).unwrap().into_raw() as *mut u8,
    }
}

#[unsafe(no_mangle)]
pub extern "C" 
fn ngenrs_z_compress(
    input: *const u8,
    input_len: usize,
    output: *mut *mut u8,
    output_len: *mut usize,
    format: c_int,
) -> *mut u8 {
    _ngenrs_z_process(format, input, input_len, output, output_len, compress)
}

#[unsafe(no_mangle)]
pub extern "C" 
fn ngenrs_z_decompress(
    input: *const u8,
    input_len: usize,
    output: *mut *mut u8,
    output_len: *mut usize,
    format: c_int,
) -> *mut u8 {
    _ngenrs_z_process(format, input, input_len, output, output_len, decompress)
}