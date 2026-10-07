use crate::DynrsStatus;
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

/// Writes a failure message into `err_out` and returns the status.
///
/// The message is sanitised first and the literal has no NUL, so the conversion cannot fail;
/// recovering rather than panicking keeps the invariant from turning into an abort at the C
/// boundary.
fn failed(err_out: *mut *mut u8, message: &str) -> DynrsStatus {
    if !err_out.is_null() {
        let sanitized = message.replace('\0', " ");
        let text = CString::new(sanitized)
            .unwrap_or_else(|_| CString::new("Invalid buffer").expect("the literal has no NUL"));
        unsafe { *err_out = text.into_raw() as *mut u8 };
    }
    DynrsStatus::Failed
}

/// Shared by the two entry points. It is deliberately *not* wrapped in `ffi_return!`: it is private,
/// and each public function wraps its own call, so the guard sits on the boundary rather than one
/// frame inside it.
///
/// The status carries the outcome and `err_out` the message, which used to travel the other way
/// round: a *non-null* return meant failure, so a caller had to remember that a successful call
/// hands back null and a failed one hands back something that looks like data.
fn _ngenrs_z_process(
    format: c_int,
    input: *const u8,
    input_len: usize,
    output: *mut *mut u8,
    output_len: *mut usize,
    err_out: *mut *mut u8,
    operation: ZipOperation,
) -> DynrsStatus {
    let Some(format) = z_format(format) else {
        return failed(err_out, "Unknown compression format");
    };

    if input.is_null() || output.is_null() || output_len.is_null() {
        return DynrsStatus::InvalidArgument;
    }

    let Some(input_slice) = cbytes_to_rust(input, input_len) else {
        return DynrsStatus::InvalidArgument;
    };

    match operation(std::io::Cursor::new(input_slice), format) {
        Ok(result) => {
            let (ptr, len) = rust_to_cbytes(result);
            unsafe {
                *output = ptr;
                *output_len = len;
            }
            DynrsStatus::Ok
        }
        Err(e) => failed(err_out, &e.to_string()),
    }
}

/// Compresses `input`, writing the buffer through `output` and its length through `output_len`.
///
/// Release a non-null buffer with `ngenrs_free_bytes(ptr)`. A failure message, when one is written,
/// is released with `ngenrs_free_cstr`.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_z_compress(
    input: *const u8,
    input_len: usize,
    output: *mut *mut u8,
    output_len: *mut usize,
    format: c_int,
    err_out: *mut *mut u8,
) -> DynrsStatus {
    ffi_return! {
        _ngenrs_z_process(
            format, input, input_len, output, output_len, err_out, compress,
        )
    }
}

/// Decompresses `input`. See [`ngenrs_z_compress`] for the buffers and their release.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_z_decompress(
    input: *const u8,
    input_len: usize,
    output: *mut *mut u8,
    output_len: *mut usize,
    format: c_int,
    err_out: *mut *mut u8,
) -> DynrsStatus {
    ffi_return! {
        _ngenrs_z_process(
            format, input, input_len, output, output_len, err_out, decompress,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::c::util::{ngenrs_free_bytes, ngenrs_free_cstr};
    use std::ffi::CStr;
    use std::os::raw::c_char;

    /// The shape both entry points have, so the helper below can take either of them.
    type ZEntryPoint = extern "C" fn(
        *const u8,
        usize,
        *mut *mut u8,
        *mut usize,
        c_int,
        *mut *mut u8,
    ) -> DynrsStatus;

    /// Calls an entry point and copies the result out, releasing the buffer and any error message
    /// the way a C caller would.
    fn run(
        entry: ZEntryPoint,
        data: *const u8,
        data_len: usize,
        format: c_int,
    ) -> Result<Vec<u8>, String> {
        let mut output: *mut u8 = std::ptr::null_mut();
        let mut output_len: usize = 0;
        let mut error: *mut u8 = std::ptr::null_mut();

        match entry(
            data,
            data_len,
            &mut output,
            &mut output_len,
            format,
            &mut error,
        ) {
            DynrsStatus::Ok => {
                assert!(error.is_null(), "a successful call writes no message");
                if output.is_null() {
                    return Ok(Vec::new());
                }
                let bytes = unsafe { std::slice::from_raw_parts(output, output_len) }.to_vec();
                ngenrs_free_bytes(output);
                Ok(bytes)
            }
            status => {
                assert!(
                    output.is_null() && output_len == 0,
                    "nothing is written when the call fails"
                );
                let message = if error.is_null() {
                    format!("{status:?} with no message")
                } else {
                    let text = unsafe { CStr::from_ptr(error as *const c_char) }
                        .to_string_lossy()
                        .to_string();
                    ngenrs_free_cstr(error as *mut c_char);
                    text
                };
                Err(message)
            }
        }
    }

    /// The same, for a slice that is always valid.
    fn run_bytes(entry: ZEntryPoint, data: &[u8], format: c_int) -> Result<Vec<u8>, String> {
        run(entry, data.as_ptr(), data.len(), format)
    }

    #[test]
    fn every_format_round_trips_through_the_c_abi() {
        let data = b"the quick brown fox jumps over the lazy dog".repeat(10);

        for format in [0, 1, 2] {
            let compressed = run_bytes(ngenrs_z_compress, &data, format).expect("data compresses");
            assert!(!compressed.is_empty());

            // The indexes follow DynXX's `DynXXZFormat`: 0 is zlib, 1 is gzip.
            match format {
                0 => assert_eq!(compressed[0] & 0x0f, 8, "format 0 is zlib"),
                1 => assert_eq!(&compressed[..2], &[0x1f, 0x8b][..], "format 1 is gzip"),
                _ => {}
            }

            let restored =
                run_bytes(ngenrs_z_decompress, &compressed, format).expect("data decompresses");
            assert_eq!(restored, data, "format {format} must restore the input");
        }
    }

    #[test]
    fn unknown_formats_and_null_buffers_report_an_error() {
        let data = b"payload";

        // An unknown format index must be an error, not a silent success.
        assert!(run_bytes(ngenrs_z_compress, data, 9).is_err());

        // Data that is not a stream of the requested format fails as well.
        assert!(run_bytes(ngenrs_z_decompress, &[0xff; 32], 1).is_err());

        // A null input pointer, or one with no length, is a rejected argument rather than a failure
        // of the codec: the two used to be the same error string.
        let mut output: *mut u8 = std::ptr::null_mut();
        let mut output_len: usize = 0;
        let mut error: *mut u8 = std::ptr::null_mut();
        assert_eq!(
            ngenrs_z_compress(
                std::ptr::null(),
                0,
                &mut output,
                &mut output_len,
                0,
                &mut error
            ),
            DynrsStatus::InvalidArgument
        );
        assert!(output.is_null() && output_len == 0);
        assert!(error.is_null(), "a rejected argument needs no message");

        // So are null output pointers.
        assert_eq!(
            ngenrs_z_compress(
                data.as_ptr(),
                data.len(),
                std::ptr::null_mut(),
                &mut output_len,
                0,
                &mut error
            ),
            DynrsStatus::InvalidArgument
        );
        assert_eq!(
            ngenrs_z_compress(
                data.as_ptr(),
                data.len(),
                &mut output,
                std::ptr::null_mut(),
                0,
                &mut error
            ),
            DynrsStatus::InvalidArgument
        );

        // An unknown format does carry a message, because the caller cannot tell from the status
        // alone which indexes are accepted.
        assert_eq!(
            ngenrs_z_compress(
                data.as_ptr(),
                data.len(),
                &mut output,
                &mut output_len,
                9,
                &mut error
            ),
            DynrsStatus::Failed
        );
        assert!(!error.is_null(), "an unknown format is explained");
        ngenrs_free_cstr(error as *mut c_char);
    }

    #[test]
    fn an_empty_payload_produces_a_stream_that_round_trips() {
        let compressed = run_bytes(ngenrs_z_compress, &[], 1).expect("empty data compresses");
        assert!(
            !compressed.is_empty(),
            "gzip of nothing still carries a header and a trailer"
        );

        let restored =
            run_bytes(ngenrs_z_decompress, &compressed, 1).expect("empty stream decompresses");
        assert!(restored.is_empty());
    }
}
