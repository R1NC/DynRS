use crate::c::util::{cbytes_to_rust, rust_to_cbytes};
use crate::core::zip::{CompressionFormat, compress, decompress};
use std::ffi::CString;
use std::io;
use std::os::raw::c_int;

/// The compress or decompress function a `_ngenrs_z_process` call runs.
type ZipOperation = fn(std::io::Cursor<&'static [u8]>, CompressionFormat) -> io::Result<Vec<u8>>;

/// Maps `DynXXZFormat` onto the core enum: `ZLib = 0`, `GZip = 1`, `Raw = 2`.
fn z_format(format: c_int) -> Option<CompressionFormat> {
    match format {
        0 => Some(CompressionFormat::Zlib),
        1 => Some(CompressionFormat::Gzip),
        2 => Some(CompressionFormat::Raw),
        _ => None,
    }
}

/// How this module reports a failure: a C string the caller releases with `ngenrs_free_cstr`.
/// A null return means the call succeeded.
fn error_cstr(message: &str) -> *mut u8 {
    let sanitized = message.replace('\0', " ");
    CString::new(sanitized)
        .expect("the message has no interior NUL")
        .into_raw() as *mut u8
}

fn _ngenrs_z_process(
    format: c_int,
    input: *const u8,
    input_len: usize,
    output: *mut *mut u8,
    output_len: *mut usize,
    operation: ZipOperation,
) -> *mut u8 {
    let Some(format) = z_format(format) else {
        return error_cstr("Unknown compression format");
    };

    if input.is_null() || output.is_null() || output_len.is_null() {
        return error_cstr("Invalid buffer");
    }

    let Some(input_slice) = cbytes_to_rust(input, input_len) else {
        return error_cstr("Invalid input buffer");
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
        Err(e) => error_cstr(&e.to_string()),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_z_compress(
    input: *const u8,
    input_len: usize,
    output: *mut *mut u8,
    output_len: *mut usize,
    format: c_int,
) -> *mut u8 {
    _ngenrs_z_process(format, input, input_len, output, output_len, compress)
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_z_decompress(
    input: *const u8,
    input_len: usize,
    output: *mut *mut u8,
    output_len: *mut usize,
    format: c_int,
) -> *mut u8 {
    _ngenrs_z_process(format, input, input_len, output, output_len, decompress)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::c::util::{ngenrs_free_bytes, ngenrs_free_cstr};
    use std::ffi::CStr;
    use std::os::raw::c_char;

    /// The shape both entry points have, so the helper below can take either of them.
    type ZEntryPoint = extern "C" fn(*const u8, usize, *mut *mut u8, *mut usize, c_int) -> *mut u8;

    /// Calls an entry point and copies the result out, releasing the buffer and any error message
    /// the way a C caller would.
    fn run(entry: ZEntryPoint, data: &[u8], format: c_int) -> Result<Vec<u8>, String> {
        let mut output: *mut u8 = std::ptr::null_mut();
        let mut output_len: usize = 0;

        let error = entry(
            data.as_ptr(),
            data.len(),
            &mut output,
            &mut output_len,
            format,
        );
        if !error.is_null() {
            let message = unsafe { CStr::from_ptr(error as *const c_char) }
                .to_string_lossy()
                .to_string();
            ngenrs_free_cstr(error as *mut c_char);
            return Err(message);
        }

        if output.is_null() {
            return Ok(Vec::new());
        }
        let bytes = unsafe { std::slice::from_raw_parts(output, output_len) }.to_vec();
        ngenrs_free_bytes(output, output_len);
        Ok(bytes)
    }

    #[test]
    fn every_format_round_trips_through_the_c_abi() {
        let data = b"the quick brown fox jumps over the lazy dog".repeat(10);

        for format in [0, 1, 2] {
            let compressed = run(ngenrs_z_compress, &data, format).expect("data compresses");
            assert!(!compressed.is_empty());

            // The indexes follow DynXX's `DynXXZFormat`: 0 is zlib, 1 is gzip.
            match format {
                0 => assert_eq!(compressed[0] & 0x0f, 8, "format 0 is zlib"),
                1 => assert_eq!(&compressed[..2], &[0x1f, 0x8b][..], "format 1 is gzip"),
                _ => {}
            }

            let restored =
                run(ngenrs_z_decompress, &compressed, format).expect("data decompresses");
            assert_eq!(restored, data, "format {format} must restore the input");
        }
    }

    #[test]
    fn unknown_formats_and_null_buffers_report_an_error() {
        let data = b"payload";

        // An unknown format index must be an error, not a silent success.
        assert!(run(ngenrs_z_compress, data, 9).is_err());

        // Data that is not a stream of the requested format fails as well.
        assert!(run(ngenrs_z_decompress, &[0xff; 32], 1).is_err());

        // A null input pointer is an error too.
        let mut output: *mut u8 = std::ptr::null_mut();
        let mut output_len: usize = 0;
        let error = ngenrs_z_compress(std::ptr::null(), 0, &mut output, &mut output_len, 0);
        assert!(!error.is_null(), "a null input must be reported");
        assert!(
            output.is_null() && output_len == 0,
            "nothing is written when the call fails"
        );
        ngenrs_free_cstr(error as *mut c_char);

        // So are null output pointers.
        let error = ngenrs_z_compress(
            data.as_ptr(),
            data.len(),
            std::ptr::null_mut(),
            &mut output_len,
            0,
        );
        assert!(!error.is_null());
        ngenrs_free_cstr(error as *mut c_char);

        let error = ngenrs_z_compress(
            data.as_ptr(),
            data.len(),
            &mut output,
            std::ptr::null_mut(),
            0,
        );
        assert!(!error.is_null());
        ngenrs_free_cstr(error as *mut c_char);
    }

    #[test]
    fn an_empty_payload_produces_a_stream_that_round_trips() {
        let compressed = run(ngenrs_z_compress, &[], 1).expect("empty data compresses");
        assert!(
            !compressed.is_empty(),
            "gzip of nothing still carries a header and a trailer"
        );

        let restored = run(ngenrs_z_decompress, &compressed, 1).expect("empty stream decompresses");
        assert!(restored.is_empty());
    }
}
