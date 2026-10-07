use crate::DynrsStatus;
use crate::c::util::{
    box_into_raw_new, bytes_to_c, cstr_to_rust, ngenrs_free_ptr, rust_map_from_c_arrays,
    rust_map_to_c_arrays, rust_to_cstr_lossy,
};
use crate::core::net::{
    ConfigError, DnsConfig, HttpClient, HttpResponse, NetError, UploadPart, set_cert_path,
    set_dns_configs, set_proxy,
};
use std::collections::HashMap;
use std::os::raw::{c_char, c_void};
use std::path::Path;
use std::slice;

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_http_client_init(ca_cert_path: *const c_char) -> *mut c_void {
    ffi_return! {
        // A path that is not valid UTF-8 is refused instead of panicking on it.
        let ca_path = if ca_cert_path.is_null() {
            None
        } else {
            match cstr_to_rust(ca_cert_path) {
                Some(path) => Some(Path::new(path)),
                None => return std::ptr::null_mut(),
            }
        };

        // A certificate path that cannot be read or parsed is a configuration error the caller
        // reports by getting null, which is what the other `_open`/`_init` entry points do.
        match HttpClient::new(ca_path) {
            Ok(client) => box_into_raw_new(client) as *mut c_void,
            Err(_) => std::ptr::null_mut(),
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_http_client_release(client: *mut c_void) {
    ffi_return! {
        if !client.is_null() {
            unsafe { drop(Box::from_raw(client as *mut HttpClient)) };
        }
    }
}

/// Mirrors `DynXXHttpProxyConfig`.
#[repr(C)]
pub struct NgenrsHttpProxyConfig {
    /// Proxy host, null or empty means no proxy.
    pub host: *const c_char,
    /// Proxy port, 0 means not appended to the host.
    pub port: usize,
    /// Proxy auth username, optional.
    pub username: *const c_char,
    /// Proxy auth password, optional.
    pub password: *const c_char,
}

/// Mirrors `DynXXHttpDnsConfig`.
#[repr(C)]
pub struct NgenrsHttpDnsConfig {
    /// Host name to override.
    pub host: *const c_char,
    /// Port, 0 means any port.
    pub port: usize,
    /// Fixed IP address.
    pub address: *const c_char,
}

/// Maps a refused configuration onto the status the C caller sees.
fn config_status(error: ConfigError) -> DynrsStatus {
    match error {
        ConfigError::InvalidUtf8 => DynrsStatus::InvalidArgument,
        ConfigError::InvalidAddress => DynrsStatus::InvalidArgument,
        ConfigError::InvalidPort => DynrsStatus::InvalidArgument,
    }
}

/// Mirrors `dynxx_net_http_set_cert_path`: a null or empty path disables peer verification, any
/// other value is used as the CA bundle path.
///
/// A path that cannot be read as UTF-8 is [`DynrsStatus::InvalidArgument`] and changes nothing.
/// Reading it as "disable verification" — which is what collapsing it into the null case used to do
/// — would turn a bad argument into a silent security downgrade.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_net_http_set_cert_path(path: *const c_char) -> DynrsStatus {
    ffi_return! {
        let path = if path.is_null() {
            Some("")
        } else {
            match cstr_to_rust(path) {
                Some(path) => Some(path),
                None => return DynrsStatus::InvalidArgument,
            }
        };
        match set_cert_path(path) {
            Ok(()) => DynrsStatus::Ok,
            Err(error) => config_status(error),
        }
    }
}

/// Mirrors `dynxx_net_http_set_proxy`: a null pointer or an empty host clears the proxy.
///
/// `port` is a `size_t` like DynXX's. A value above `u16::MAX` is refused rather than silently
/// becoming 0, because 0 means "do not append a port" and that is a different request.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_net_http_set_proxy(proxy: *const NgenrsHttpProxyConfig) -> DynrsStatus {
    ffi_return! {
        let Some(proxy) = (unsafe { proxy.as_ref() }) else {
            return match set_proxy(Some(""), 0, "", "") {
                Ok(()) => DynrsStatus::Ok,
                Err(error) => config_status(error),
            };
        };

        let Some(port) = u16::try_from(proxy.port).ok() else {
            return DynrsStatus::InvalidArgument;
        };
        // A username or password that is not UTF-8 is refused: silently dropping it would send the
        // request without the credentials the caller asked for.
        let username = cstr_to_rust(proxy.username).unwrap_or_default();
        let password = cstr_to_rust(proxy.password).unwrap_or_default();
        if !proxy.username.is_null() && username.is_empty() {
            return DynrsStatus::InvalidArgument;
        }
        if !proxy.password.is_null() && password.is_empty() {
            return DynrsStatus::InvalidArgument;
        }

        match set_proxy(cstr_to_rust(proxy.host), port, username, password) {
            Ok(()) => DynrsStatus::Ok,
            Err(error) => config_status(error),
        }
    }
}

/// Mirrors `dynxx_net_http_set_dns_configs`: a null pointer or a zero count clears the overrides.
///
/// An entry the core cannot use — an empty host, an address that is not a literal IP, a host that
/// is not UTF-8 — is [`DynrsStatus::InvalidArgument`] and leaves the previous overrides in place.
/// Dropping the entry quietly is what let a caller believe a pin was in effect when it was not.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_net_http_set_dns_configs(
    configs: *const NgenrsHttpDnsConfig,
    config_count: usize,
) -> DynrsStatus {
    ffi_return! {
        if configs.is_null() || config_count == 0 {
            return match set_dns_configs(&[]) {
                Ok(()) => DynrsStatus::Ok,
                Err(error) => config_status(error),
            };
        }

        let configs = unsafe { slice::from_raw_parts(configs, config_count) };
        let mut resolved = Vec::with_capacity(configs.len());
        for config in configs {
            let (Some(host), Some(address)) =
                (cstr_to_rust(config.host), cstr_to_rust(config.address))
            else {
                return DynrsStatus::InvalidArgument;
            };
            let Some(port) = u16::try_from(config.port).ok() else {
                return DynrsStatus::InvalidArgument;
            };
            resolved.push(DnsConfig {
                host: host.to_string(),
                port,
                address: address.to_string(),
            });
        }

        match set_dns_configs(&resolved) {
            Ok(()) => DynrsStatus::Ok,
            Err(error) => config_status(error),
        }
    }
}

/// Reads a required C string argument.
///
/// A null pointer or a value that is not UTF-8 is an argument error rather than an empty string:
/// silently substituting `""` used to produce a request to a wrong URL, a write to `Path::new("")`,
/// or a stored value the caller never asked for.
fn required_str(value: *const c_char) -> Result<String, DynrsStatus> {
    if value.is_null() {
        return Err(DynrsStatus::InvalidArgument);
    }
    cstr_to_rust(value)
        .map(str::to_string)
        .ok_or(DynrsStatus::InvalidArgument)
}

/// Reads an optional C string argument: absent when the pointer is null, an error when it is
/// present but not readable.
fn optional_str(value: *const c_char) -> Result<Option<String>, DynrsStatus> {
    if value.is_null() {
        return Ok(None);
    }
    cstr_to_rust(value)
        .map(|text| Some(text.to_string()))
        .ok_or(DynrsStatus::InvalidArgument)
}

/// Borrows a client handle, or reports it as invalid.
fn client_ref<'a>(client: *const c_void) -> Result<&'a HttpClient, DynrsStatus> {
    unsafe { (client as *const HttpClient).as_ref() }.ok_or(DynrsStatus::InvalidHandle)
}

/// Hands a finished request back through `out`, mapping [`NetError`] onto a status.
///
/// Each request entry point used to repeat this match — three arms, the boxing, and the out-parameter
/// check — and had to know that `None` from the runtime meant the same thing as a failed request.
/// What is left to the caller is reading arguments and naming the request.
fn respond(out: *mut *mut c_void, result: Result<HttpResponse, NetError>) -> DynrsStatus {
    match result {
        Ok(response) => {
            if !out.is_null() {
                unsafe { *out = box_into_raw_new(response) as *mut c_void };
            }
            DynrsStatus::Ok
        }
        // The reason is not reported here: a response handle is the only result this call hands
        // back, and a caller that needs to know why can read it from the status of the sub-call
        // that failed. `Request` is kept distinct from `Internal` in the core type so a future
        // entry point can tell "your input was wrong" from "the network failed" without a string.
        Err(NetError::Internal(_)) => DynrsStatus::InvalidArgument,
        Err(_) => DynrsStatus::Failed,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_http_get(
    client: *const c_void,
    url: *const c_char,
    header_keys: *const *const c_char,
    header_values: *const *const c_char,
    headers_len: usize,
    body: *const c_char,
    out: *mut *mut c_void,
) -> DynrsStatus {
    ffi_return! {
        let client = match client_ref(client) {
            Ok(client) => client,
            Err(status) => return status,
        };
        let url = match required_str(url) {
            Ok(url) => url,
            Err(status) => return status,
        };
        let body = match optional_str(body) {
            Ok(body) => body,
            Err(status) => return status,
        };
        let headers = unsafe { rust_map_from_c_arrays(header_keys, header_values, headers_len) };

        respond(out, client.blocking_get(&url, headers, body.as_deref()))
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_http_post(
    client: *const c_void,
    url: *const c_char,
    header_keys: *const *const c_char,
    header_values: *const *const c_char,
    headers_len: usize,
    body: *const c_char,
    json_keys: *const *const c_char,
    json_values: *const *const c_char,
    json_len: usize,
    out: *mut *mut c_void,
) -> DynrsStatus {
    ffi_return! {
        let client = match client_ref(client) {
            Ok(client) => client,
            Err(status) => return status,
        };
        let url = match required_str(url) {
            Ok(url) => url,
            Err(status) => return status,
        };
        let body = match optional_str(body) {
            Ok(body) => body,
            Err(status) => return status,
        };
        let headers = unsafe { rust_map_from_c_arrays(header_keys, header_values, headers_len) };
        let json_map = unsafe { rust_map_from_c_arrays(json_keys, json_values, json_len) };

        respond(
            out,
            client.blocking_post(&url, headers, body.as_deref(), json_map),
        )
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_http_download(
    client: *const c_void,
    url: *const c_char,
    header_keys: *const *const c_char,
    header_values: *const *const c_char,
    headers_len: usize,
    output_path: *const c_char,
    out: *mut *mut c_void,
) -> DynrsStatus {
    ffi_return! {
        let client = match client_ref(client) {
            Ok(client) => client,
            Err(status) => return status,
        };
        let url = match required_str(url) {
            Ok(url) => url,
            Err(status) => return status,
        };
        // An empty path would have written the response to a file the caller never named.
        let output_path = match required_str(output_path) {
            Ok(path) if !path.is_empty() => path,
            _ => return DynrsStatus::InvalidArgument,
        };
        let headers = unsafe { rust_map_from_c_arrays(header_keys, header_values, headers_len) };

        let path = Path::new(&output_path);
        respond(out, client.blocking_download(&url, headers, path))
    }
}

/// Sends a multipart form.
///
/// `parts` points at `parts_len` values of [`NgenrsHttpPart`]. That replaced five parallel arrays
/// whose lengths had to agree with each other and with `parts_len`: nothing enforced it, so a
/// mismatched pair was read as a memory error rather than refused as an argument.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_http_upload(
    client: *const c_void,
    url: *const c_char,
    header_keys: *const *const c_char,
    header_values: *const *const c_char,
    headers_len: usize,
    parts: *const NgenrsHttpPart,
    parts_len: usize,
    out: *mut *mut c_void,
) -> DynrsStatus {
    ffi_return! {
        let client = match client_ref(client) {
            Ok(client) => client,
            Err(status) => return status,
        };
        let url = match required_str(url) {
            Ok(url) => url,
            Err(status) => return status,
        };
        let headers = unsafe { rust_map_from_c_arrays(header_keys, header_values, headers_len) };

        // The parts are read through a checked view instead of `slice::from_raw_parts`, which is
        // undefined behaviour for a null pointer even at length zero and cannot tell a lying length
        // from a real one.
        let Some(parts_array) = SlicePtr::new(parts, parts_len) else {
            return DynrsStatus::InvalidArgument;
        };

        // A part the caller did not name, or one whose data pointer is null while claiming a
        // length, is a broken description of the request. Dropping it used to send the rest and
        // report success, which is how a caller came to believe a part had been uploaded.
        let mut collected: Vec<UploadPart> = Vec::with_capacity(parts_len);
        for index in 0..parts_len {
            let Some(part) = (unsafe { parts_array.element(index) }) else {
                return DynrsStatus::InvalidArgument;
            };
            let Some(name) = cstr_to_rust(part.name) else {
                return DynrsStatus::InvalidArgument;
            };
            // A null data pointer is an empty field; one that claims a length without pointing at
            // anything is a lie the caller cannot have meant.
            let bytes = if part.data.is_null() {
                if part.data_len > 0 {
                    return DynrsStatus::InvalidArgument;
                }
                Vec::new()
            } else {
                unsafe { slice::from_raw_parts(part.data, part.data_len) }.to_vec()
            };
            let mime = match optional_part_str(part.mime) {
                Ok(mime) => mime,
                Err(status) => return status,
            };
            let filename = match optional_part_str(part.filename) {
                Ok(filename) => filename,
                Err(status) => return status,
            };

            // The rule about what makes a valid part belongs to the core type, so it is stated once
            // there.
            match UploadPart::new(name, bytes, mime, filename) {
                Ok(part) => collected.push(part),
                Err(_) => return DynrsStatus::InvalidArgument,
            }
        }

        respond(out, client.blocking_upload(&url, headers, collected))
    }
}

/// Reads an optional string field of [`NgenrsHttpPart`]: absent when null, an error when present
/// but not readable.
fn optional_part_str(value: *const c_char) -> Result<Option<String>, DynrsStatus> {
    if value.is_null() {
        return Ok(None);
    }
    cstr_to_rust(value)
        .map(|text| Some(text.to_string()))
        .ok_or(DynrsStatus::InvalidArgument)
}

/// Reads the HTTP status of the response into `status_out`.
///
/// The status used to be the return value with `-1` standing in for a bad handle, which a caller
/// storing it in an unsigned type would lose. It is an out-parameter now, and the return value says
/// whether there is a status at all.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_http_parse_rsp_status(
    rsp_ptr: *mut c_void,
    status_out: *mut i32,
) -> DynrsStatus {
    ffi_return! {
        let Some(rsp) = (unsafe { (rsp_ptr as *const HttpResponse).as_ref() }) else {
            return DynrsStatus::InvalidHandle;
        };
        if status_out.is_null() {
            return DynrsStatus::InvalidArgument;
        }
        unsafe { *status_out = i32::from(rsp.status.as_u16()) };
        DynrsStatus::Ok
    }
}

/// Hands out the response headers as two parallel arrays of C strings, at most `keys_cap` and
/// `values_cap` entries. `count` receives how many were written, so a caller whose arrays were too
/// small sees a smaller number than the response actually carried. Each written entry belongs to
/// the caller and is released with `ngenrs_free_cstr`.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_http_parse_rsp_headers(
    rsp_ptr: *mut c_void,
    keys: *mut *mut c_char,
    keys_cap: usize,
    values: *mut *mut c_char,
    values_cap: usize,
    count: *mut usize,
) -> DynrsStatus {
    ffi_return! {
        if !count.is_null() {
            unsafe { *count = 0 };
        }
        let Some(rsp) = (unsafe { (rsp_ptr as *const HttpResponse).as_ref() }) else {
            return DynrsStatus::InvalidHandle;
        };
        if count.is_null() {
            return DynrsStatus::InvalidArgument;
        }

        // A header name or value that is not readable text is skipped; HTTP forbids a NUL in
        // either, so this cannot silently drop something the peer sent.
        let headers_map: HashMap<String, String> = rsp
            .headers
            .iter()
            .filter_map(|(k, v)| Some((k.to_string(), v.to_str().ok()?.to_string())))
            .collect();

        // HTTP forbids a NUL in a header name or value, so a failure here means the response is not
        // something this reader can present.
        match unsafe {
            rust_map_to_c_arrays(&headers_map, keys, keys_cap, values, values_cap, count)
        } {
            Ok(_) => DynrsStatus::Ok,
            Err(_) => DynrsStatus::Failed,
        }
    }
}

/// Hands out the response body as bytes: `out` receives the buffer and `out_len` its length. The
/// caller releases a non-null buffer with `ngenrs_free_bytes(ptr)`.
///
/// The body travels as bytes rather than as a C string because it is untrusted input that may
/// legitimately contain a NUL: as a C string, a body with an interior NUL came back as a null
/// pointer, which is the same thing a response *without* a body reported.
///
/// [`DynrsStatus::Empty`] means the response has no body here — a download put it in a file — and
/// [`DynrsStatus::Failed`] means it has one that could not be read. Those two used to be the same
/// null pointer.
///
/// `err_out` carries the reason when the read failed, because a body read fails for reasons the
/// caller may be able to act on (a truncated transfer, a timeout, a connection reset) and the status
/// alone would leave them guessing. It used to be printed to stderr, which is a side effect a
/// library should not have: the caller owns the reporting, and on a platform whose stderr goes
/// nowhere the reason was simply lost. Release it with `ngenrs_free_cstr`.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_http_parse_rsp_body(
    rsp_ptr: *mut c_void,
    out: *mut *mut u8,
    out_len: *mut usize,
    err_out: *mut *mut c_char,
) -> DynrsStatus {
    ffi_return! {
        if !out.is_null() {
            unsafe { *out = std::ptr::null_mut() };
        }
        if !out_len.is_null() {
            unsafe { *out_len = 0 };
        }
        if !err_out.is_null() {
            unsafe { *err_out = std::ptr::null_mut() };
        }
        let Some(rsp) = (unsafe { (rsp_ptr as *const HttpResponse).as_ref() }) else {
            return DynrsStatus::InvalidHandle;
        };
        if out.is_null() {
            return DynrsStatus::InvalidArgument;
        }

        if let Some(error) = rsp.body_error.as_ref() {
            if !err_out.is_null() {
                unsafe { *err_out = rust_to_cstr_lossy(error.clone()) };
            }
            return DynrsStatus::Failed;
        }

        match rsp.body.as_ref() {
            Some(body) => {
                unsafe { *out = bytes_to_c(body.as_bytes().to_vec(), out_len) };
                DynrsStatus::Ok
            }
            None => DynrsStatus::Empty,
        }
    }
}

/// Releases a response of ngenrs_http_get`, ngenrs_http_post`, ngenrs_http_download` or
/// ngenrs_http_upload`. The `parse_rsp_*` readers only look at the response, so the caller has to
/// hand it back once it is done with it.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_http_release_rsp(rsp_ptr: *mut c_void) {
    ffi_return! {
        if !rsp_ptr.is_null() {
            ngenrs_free_ptr(rsp_ptr as *mut HttpResponse);
        }
    }
}

/// A caller-supplied array of `*const T`, read element by element.
///
/// `slice::from_raw_parts` is undefined behaviour for a null pointer even at length zero, and it
/// trusts the length it is given. `ngenrs_http_upload` receives the caller's parts as an array whose
/// length is part of its contract, so it is wrapped here and read with a bound check instead.
struct SlicePtr<T> {
    ptr: *const T,
    len: usize,
}

impl<T: Copy> SlicePtr<T> {
    /// `None` when there is no array to read: a null pointer with a non-zero length.
    fn new(ptr: *const T, len: usize) -> Option<Self> {
        if len == 0 {
            return Some(Self {
                ptr: std::ptr::NonNull::<T>::dangling().as_ptr(),
                len: 0,
            });
        }
        if ptr.is_null() {
            return None;
        }
        Some(Self { ptr, len })
    }

    /// The element at `index`, or `None` when the index is past the end.
    unsafe fn element(&self, index: usize) -> Option<T> {
        if index >= self.len {
            return None;
        }
        Some(unsafe { *self.ptr.add(index) })
    }
}

/// One multipart field of an upload, as the C caller describes it.
///
/// A zero-initialised value is a valid "empty part", which the entry point then refuses for having
/// no name. `name`, `mime` and `filename` are NUL-terminated strings, and `mime` and `filename` may
/// be null when the caller has nothing to say; `data` with `data_len` of zero is an empty field.
///
/// This replaces five parallel arrays (names, data, data lengths, MIME types, file names) whose
/// lengths had to agree. Nothing enforced that agreement, and reading the wrong pair was a memory
/// error rather than a rejected argument.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct NgenrsHttpPart {
    pub name: *const c_char,
    pub data: *const u8,
    pub data_len: usize,
    pub mime: *const c_char,
    pub filename: *const c_char,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::c::util::{ngenrs_free_bytes, ngenrs_free_cstr};
    use crate::core::net::CertConfig;
    use crate::core::net::{CONFIG_TEST_LOCK, config_snapshot};
    use crate::core::net_test_server::{Server, behind_an_environment_proxy, ok};
    use std::ffi::{CStr, CString};
    use std::sync::MutexGuard;

    fn lock_config() -> MutexGuard<'static, ()> {
        CONFIG_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// A certificate path whose bytes are not valid UTF-8 used to panic in `cstr_to_rust(..)
    /// .unwrap()`, and a path that cannot be read used to panic in `HttpClient::new(..).expect(..)`.
    /// Both are ordinary configuration errors: they abort the host when they escape the C ABI, and
    /// the caller is meant to see a null handle.
    #[test]
    fn a_client_whose_configuration_is_rejected_comes_back_as_null() {
        // 0x80 is a continuation byte with no lead byte, so this is not UTF-8.
        let not_utf8 = [0x2fu8, 0x74u8, 0x6du8, 0x70u8, 0x80u8, 0x00u8];
        assert!(
            ngenrs_http_client_init(not_utf8.as_ptr().cast()).is_null(),
            "a path that is not UTF-8 is refused"
        );

        let missing = CString::new("/no/such/directory/ca.pem").unwrap();
        assert!(
            ngenrs_http_client_init(missing.as_ptr()).is_null(),
            "a certificate file that cannot be read is refused"
        );

        let not_pem = CString::new("/etc/hostname").unwrap();
        let client = ngenrs_http_client_init(not_pem.as_ptr());
        if !client.is_null() {
            // On a host where that path exists and happens to parse, the handle is still valid.
            ngenrs_http_client_release(client);
        }
    }

    /// A body is untrusted input and may contain a NUL. As a C string it was reported as null,
    /// which is the same thing a response *without* a body reported, and the length-based form is
    /// what makes the two distinguishable.
    #[test]
    fn a_body_with_an_interior_nul_still_arrives_whole() {
        let Some((client, _guard)) = client_and_clean_config() else {
            return;
        };

        let body = "a\0b";
        let server = Server::start(format!(
            "HTTP/1.1 200 OK\r\ncontent-length: {}\r\n\r\n{body}",
            body.len()
        ));
        let url = CString::new(server.url("/nul")).unwrap();
        let mut response = std::ptr::null_mut();
        assert_eq!(
            ngenrs_http_get(
                client,
                url.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                std::ptr::null(),
                &mut response,
            ),
            DynrsStatus::Ok
        );
        assert!(!response.is_null(), "the request went through");

        let mut parsed = std::ptr::null_mut();
        let mut len = 0usize;
        let mut error = std::ptr::null_mut();
        assert_eq!(
            ngenrs_http_parse_rsp_body(response, &mut parsed, &mut len, &mut error),
            DynrsStatus::Ok
        );
        assert!(!parsed.is_null(), "a body with a NUL is still a body");
        assert_eq!(len, body.len());
        assert_eq!(
            unsafe { std::slice::from_raw_parts(parsed, len) },
            body.as_bytes()
        );
        ngenrs_free_bytes(parsed);

        ngenrs_http_release_rsp(response);
        ngenrs_http_client_release(client);
    }

    /// Every request entry point refuses a null handle instead of turning it into a reference, and
    /// says *that* rather than reporting a failed request.
    #[test]
    fn a_null_client_is_refused_by_every_request_entry_point() {
        let url = CString::new("http://127.0.0.1:1/").unwrap();
        let mut out = std::ptr::null_mut();

        assert_eq!(
            ngenrs_http_get(
                std::ptr::null(),
                url.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                std::ptr::null(),
                &mut out,
            ),
            DynrsStatus::InvalidHandle
        );
        assert_eq!(
            ngenrs_http_post(
                std::ptr::null(),
                url.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                &mut out,
            ),
            DynrsStatus::InvalidHandle
        );
        assert_eq!(
            ngenrs_http_download(
                std::ptr::null(),
                url.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                url.as_ptr(),
                &mut out,
            ),
            DynrsStatus::InvalidHandle
        );
        assert_eq!(
            ngenrs_http_upload(
                std::ptr::null(),
                url.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
                &mut out,
            ),
            DynrsStatus::InvalidHandle
        );
        assert!(out.is_null(), "a refused request produces no handle");
    }

    /// The header arrays bound what is written: a caller with room for one entry gets one and a
    /// count that says so. Before the capacity arguments existed this wrote every header of the
    /// response, however many that was, straight past the end of the array.
    #[test]
    fn header_arrays_are_bounded_by_the_capacity_the_caller_passes() {
        let Some((client, _guard)) = client_and_clean_config() else {
            return;
        };

        let server = Server::start(ok("hello"));
        let url = CString::new(server.url("/bounded")).unwrap();
        let mut response = std::ptr::null_mut();
        assert_eq!(
            ngenrs_http_get(
                client,
                url.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                std::ptr::null(),
                &mut response,
            ),
            DynrsStatus::Ok
        );
        assert!(!response.is_null(), "the request went through");

        // Room for one, while the response certainly carries more than one header.
        let mut keys = [std::ptr::null_mut(); 1];
        let mut values = [std::ptr::null_mut(); 1];
        let mut count = usize::MAX;
        assert_eq!(
            ngenrs_http_parse_rsp_headers(
                response,
                keys.as_mut_ptr(),
                keys.len(),
                values.as_mut_ptr(),
                values.len(),
                &mut count,
            ),
            DynrsStatus::Ok
        );

        assert_eq!(count, 1, "only as many entries as the arrays hold");
        assert!(!keys[0].is_null());
        assert!(!values[0].is_null());
        ngenrs_free_cstr(keys[0]);
        ngenrs_free_cstr(values[0]);

        ngenrs_http_release_rsp(response);
        ngenrs_http_client_release(client);
    }

    /// A client over a clean global config, with the lock that keeps it stable, or `None` when a
    /// proxy in the environment would keep the requests away from this machine anyway.
    fn client_and_clean_config() -> Option<(*mut c_void, MutexGuard<'static, ()>)> {
        if behind_an_environment_proxy() {
            return None;
        }

        let guard = lock_config();
        // A clean config, through the entry points a C caller would use as well.
        assert_eq!(
            ngenrs_net_http_set_cert_path(std::ptr::null()),
            DynrsStatus::Ok
        );
        assert_eq!(ngenrs_net_http_set_proxy(std::ptr::null()), DynrsStatus::Ok);
        assert_eq!(
            ngenrs_net_http_set_dns_configs(std::ptr::null(), 0),
            DynrsStatus::Ok
        );

        let client = ngenrs_http_client_init(std::ptr::null());
        assert!(!client.is_null(), "the client is created");
        Some((client, guard))
    }

    /// The whole path a C caller takes: configure, request, read the response, release it.
    #[test]
    fn a_request_round_trips_through_the_c_abi() {
        let Some((client, _guard)) = client_and_clean_config() else {
            return;
        };

        let server = Server::start(ok("hello"));
        let url = CString::new(server.url("/c")).unwrap();
        let key = CString::new("x-request").unwrap();
        let value = CString::new("1").unwrap();
        let body = CString::new("payload").unwrap();
        let keys = [key.as_ptr()];
        let values = [value.as_ptr()];

        let mut response = std::ptr::null_mut();
        assert_eq!(
            ngenrs_http_get(
                client,
                url.as_ptr(),
                keys.as_ptr(),
                values.as_ptr(),
                keys.len(),
                body.as_ptr(),
                &mut response,
            ),
            DynrsStatus::Ok
        );
        assert!(!response.is_null(), "the request went through");
        let mut status = 0;
        assert_eq!(
            ngenrs_http_parse_rsp_status(response, &mut status),
            DynrsStatus::Ok
        );
        assert_eq!(status, 200);

        // The body comes back as bytes plus a length, and is released with `ngenrs_free_bytes`.
        let mut parsed = std::ptr::null_mut();
        let mut body_len = 0;
        let mut error = std::ptr::null_mut();
        assert_eq!(
            ngenrs_http_parse_rsp_body(response, &mut parsed, &mut body_len, &mut error),
            DynrsStatus::Ok
        );
        assert_eq!(
            unsafe { std::slice::from_raw_parts(parsed, body_len) },
            b"hello"
        );
        ngenrs_free_bytes(parsed);

        // The headers are handed out as strings, one `ngenrs_free_cstr` each. The arrays bound what
        // is written, so a response with more headers than this simply reports fewer.
        let mut header_keys = [std::ptr::null_mut(); 16];
        let mut header_values = [std::ptr::null_mut(); 16];
        let mut header_count = 0;
        assert_eq!(
            ngenrs_http_parse_rsp_headers(
                response,
                header_keys.as_mut_ptr(),
                header_keys.len(),
                header_values.as_mut_ptr(),
                header_values.len(),
                &mut header_count,
            ),
            DynrsStatus::Ok
        );
        assert!(header_count > 0, "the response has headers");
        let mut reply = None;
        for index in 0..header_count {
            let name = unsafe { CStr::from_ptr(header_keys[index]) }
                .to_str()
                .unwrap()
                .to_string();
            let header = unsafe { CStr::from_ptr(header_values[index]) }
                .to_str()
                .unwrap()
                .to_string();
            if name.eq_ignore_ascii_case("x-reply") {
                reply = Some(header);
            }
            ngenrs_free_cstr(header_keys[index]);
            ngenrs_free_cstr(header_values[index]);
        }
        assert_eq!(reply.as_deref(), Some("yes"));

        let request = server.request(0).to_ascii_lowercase();
        assert!(request.contains("x-request: 1"), "{request}");
        assert!(request.ends_with("payload"), "{request}");

        // A null pointer is reported as a bad handle by every reader, and the release tolerates it.
        let mut status = 0;
        assert_eq!(
            ngenrs_http_parse_rsp_status(std::ptr::null_mut(), &mut status),
            DynrsStatus::InvalidHandle
        );
        assert_eq!(
            ngenrs_http_parse_rsp_body(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut()
            ),
            DynrsStatus::InvalidHandle
        );
        assert_eq!(
            ngenrs_http_parse_rsp_headers(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
            ),
            DynrsStatus::InvalidHandle
        );
        ngenrs_http_release_rsp(std::ptr::null_mut());

        ngenrs_http_release_rsp(response);
        ngenrs_http_client_release(client);
    }

    /// The JSON arguments become the request body; a value that is not JSON is dropped.
    #[test]
    fn post_sends_json_params_through_the_c_abi() {
        let Some((client, _guard)) = client_and_clean_config() else {
            return;
        };
        let server = Server::start(ok("taken"));

        let url = CString::new(server.url("/json")).unwrap();
        let key = CString::new("a").unwrap();
        let value = CString::new("1").unwrap();
        let broken = CString::new("not json").unwrap();
        let json_keys = [key.as_ptr(), broken.as_ptr()];
        let json_values = [value.as_ptr(), broken.as_ptr()];

        let mut response = std::ptr::null_mut();
        assert_eq!(
            ngenrs_http_post(
                client,
                url.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                std::ptr::null(),
                json_keys.as_ptr(),
                json_values.as_ptr(),
                json_keys.len(),
                &mut response,
            ),
            DynrsStatus::Ok
        );
        assert!(!response.is_null(), "the request went through");
        let mut status = 0;
        assert_eq!(
            ngenrs_http_parse_rsp_status(response, &mut status),
            DynrsStatus::Ok
        );
        assert_eq!(status, 200);

        let request = server.request(0);
        let lowered = request.to_ascii_lowercase();
        assert!(
            lowered.contains("content-type: application/json"),
            "{request}"
        );
        assert!(request.contains("\"a\":1"), "{request}");
        assert!(!lowered.contains("not json"), "{request}");

        ngenrs_http_release_rsp(response);
        ngenrs_http_client_release(client);
    }

    /// Without JSON arguments, the body is sent as it came in.
    #[test]
    fn post_sends_the_raw_body_through_the_c_abi() {
        let Some((client, _guard)) = client_and_clean_config() else {
            return;
        };
        let server = Server::start(ok("taken"));

        let url = CString::new(server.url("/raw")).unwrap();
        let body = CString::new("plain").unwrap();
        let mut response = std::ptr::null_mut();
        assert_eq!(
            ngenrs_http_post(
                client,
                url.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                body.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                &mut response,
            ),
            DynrsStatus::Ok
        );
        assert!(!response.is_null(), "the request went through");

        let request = server.request(0);
        assert!(request.ends_with("plain"), "{request}");
        assert!(
            !request.to_ascii_lowercase().contains("application/json"),
            "{request}"
        );

        ngenrs_http_release_rsp(response);
        ngenrs_http_client_release(client);
    }

    /// A download puts the body in the file instead of in the response.
    #[test]
    fn download_writes_the_file_through_the_c_abi() {
        let Some((client, _guard)) = client_and_clean_config() else {
            return;
        };
        let server = Server::start(ok("hello"));

        let url = CString::new(server.url("/file")).unwrap();
        let path =
            std::env::temp_dir().join(format!("dynrs_c_download_{}.txt", std::process::id()));
        let output = CString::new(path.to_str().unwrap()).unwrap();

        let mut response = std::ptr::null_mut();
        assert_eq!(
            ngenrs_http_download(
                client,
                url.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                output.as_ptr(),
                &mut response,
            ),
            DynrsStatus::Ok
        );
        assert!(!response.is_null(), "the request went through");
        let mut status = 0;
        assert_eq!(
            ngenrs_http_parse_rsp_status(response, &mut status),
            DynrsStatus::Ok
        );
        assert_eq!(status, 200);
        // A download puts the body in the file, so the response carries none: `Empty`, which is a
        // different answer from the `Failed` a body that could not be read produces.
        let mut parsed = std::ptr::null_mut();
        let mut body_len = 0;
        let mut error = std::ptr::null_mut();
        assert_eq!(
            ngenrs_http_parse_rsp_body(response, &mut parsed, &mut body_len, &mut error),
            DynrsStatus::Empty,
            "the body of a download is the file, not a string in the response"
        );
        assert!(parsed.is_null());
        assert_eq!(body_len, 0);
        assert_eq!(
            std::fs::read_to_string(&path).expect("the file was written"),
            "hello"
        );
        std::fs::remove_file(&path).expect("the file is removed again");

        assert!(server.request(0).starts_with("GET /file"));
        ngenrs_http_release_rsp(response);
        ngenrs_http_client_release(client);
    }

    /// A part with every optional field left out, which is what a C caller's zero-initialised value
    /// looks like. `data` is null with a length of zero, i.e. an empty field.
    fn empty_part() -> NgenrsHttpPart {
        NgenrsHttpPart {
            name: std::ptr::null(),
            data: std::ptr::null(),
            data_len: 0,
            mime: std::ptr::null(),
            filename: std::ptr::null(),
        }
    }

    /// Every part is sent with the mime type and file name it was given, and a part may leave both
    /// out.
    #[test]
    fn upload_sends_parts_through_the_c_abi() {
        let Some((client, _guard)) = client_and_clean_config() else {
            return;
        };
        let server = Server::start(ok("stored"));

        let url = CString::new(server.url("/up")).unwrap();
        let first = CString::new("first").unwrap();
        let second = CString::new("second").unwrap();
        let mime = CString::new("text/plain").unwrap();
        let filename = CString::new("a.txt").unwrap();
        let data_first = b"one".to_vec();
        let data_second = b"two".to_vec();

        let parts = [
            NgenrsHttpPart {
                name: first.as_ptr(),
                data: data_first.as_ptr(),
                data_len: data_first.len(),
                mime: mime.as_ptr(),
                filename: filename.as_ptr(),
            },
            // The second part leaves both optional strings out, which a zero-initialised value does.
            NgenrsHttpPart {
                name: second.as_ptr(),
                data: data_second.as_ptr(),
                data_len: data_second.len(),
                ..empty_part()
            },
        ];

        let mut response = std::ptr::null_mut();
        assert_eq!(
            ngenrs_http_upload(
                client,
                url.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                parts.as_ptr(),
                parts.len(),
                &mut response,
            ),
            DynrsStatus::Ok
        );
        assert!(!response.is_null(), "the request went through");
        let mut status = 0;
        assert_eq!(
            ngenrs_http_parse_rsp_status(response, &mut status),
            DynrsStatus::Ok
        );
        assert_eq!(status, 200);

        let request = server.request(0);
        let lowered = request.to_ascii_lowercase();
        assert!(
            lowered.contains("content-type: multipart/form-data; boundary="),
            "{request}"
        );
        assert!(request.contains("name=\"first\""), "{request}");
        assert!(request.contains("name=\"second\""), "{request}");
        assert!(lowered.contains("content-type: text/plain"), "{request}");
        assert_eq!(request.matches("filename=").count(), 1, "{request}");

        ngenrs_http_release_rsp(response);
        ngenrs_http_client_release(client);
    }

    /// A part that describes itself wrongly is refused before anything is sent, rather than dropped
    /// from the form: a caller that believed it uploaded a file must not be told the request
    /// succeeded.
    #[test]
    fn upload_refuses_a_broken_part_before_sending_anything() {
        let Some((client, _guard)) = client_and_clean_config() else {
            return;
        };

        // The server is never reached: each of these is refused while reading the arguments.
        let url = CString::new("http://127.0.0.1:1/up").unwrap();
        let name = CString::new("field").unwrap();
        let data = b"data".to_vec();
        let mut response = std::ptr::null_mut();

        // No name at all: a multipart field cannot be anonymous.
        let unnamed = [empty_part()];
        assert_eq!(
            ngenrs_http_upload(
                client,
                url.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                unnamed.as_ptr(),
                unnamed.len(),
                &mut response,
            ),
            DynrsStatus::InvalidArgument
        );

        // A data pointer that is null while claiming a length: the bytes are not there.
        let lying = [NgenrsHttpPart {
            name: name.as_ptr(),
            data_len: data.len(),
            ..empty_part()
        }];
        assert_eq!(
            ngenrs_http_upload(
                client,
                url.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                lying.as_ptr(),
                lying.len(),
                &mut response,
            ),
            DynrsStatus::InvalidArgument
        );

        // A non-zero part count with no array to read.
        assert_eq!(
            ngenrs_http_upload(
                client,
                url.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                std::ptr::null(),
                1,
                &mut response,
            ),
            DynrsStatus::InvalidArgument
        );

        assert!(
            response.is_null(),
            "nothing was sent, so there is no response"
        );
        ngenrs_http_client_release(client);
    }

    /// A request that cannot be made at all reports why, and leaves no handle behind.
    #[test]
    fn a_request_that_fails_gives_no_response() {
        let Some((client, _guard)) = client_and_clean_config() else {
            return;
        };
        // A URL reqwest refuses to parse is the one failure that needs no server and cannot hang:
        // the port of a listener that was just closed can be handed out again in the meantime.
        let url = CString::new("not a url").unwrap();

        let mut response = std::ptr::null_mut();
        // `InvalidArgument` rather than `Failed`: the request never left, because the URL could not
        // be turned into one. The core layer keeps those apart now, so a caller learns that retrying
        // the same URL is pointless.
        assert_eq!(
            ngenrs_http_get(
                client,
                url.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                std::ptr::null(),
                &mut response,
            ),
            DynrsStatus::InvalidArgument
        );
        assert!(
            response.is_null(),
            "the request failed, so there is no response"
        );

        ngenrs_http_client_release(client);
    }

    /// A certificate path whose bytes are not UTF-8 used to come back from `cstr_to_rust` as
    /// `None`, which `core::net::set_cert_path` read as "disable peer verification". A malformed
    /// argument therefore turned into a silent security downgrade. It is refused now, and the
    /// configuration is left exactly as it was.
    #[test]
    fn a_cert_path_that_is_not_utf8_never_disables_verification() {
        let _guard = lock_config();

        // Start from a real CA bundle, so there is something a downgrade could destroy.
        let path = CString::new("/tmp/ca.pem").unwrap();
        assert_eq!(
            ngenrs_net_http_set_cert_path(path.as_ptr()),
            DynrsStatus::Ok
        );
        assert!(matches!(config_snapshot().0, CertConfig::Path(_)));

        // 0x80 is a continuation byte with no lead byte, so this is not UTF-8.
        let not_utf8 = [0x2fu8, 0x74u8, 0x6du8, 0x70u8, 0x80u8, 0x00u8];
        assert_eq!(
            ngenrs_net_http_set_cert_path(not_utf8.as_ptr().cast()),
            DynrsStatus::InvalidArgument
        );
        assert!(
            matches!(config_snapshot().0, CertConfig::Path(_)),
            "a refused path must not fall through to disabled verification"
        );

        // A null pointer is the deliberate "do not verify" request, and still is.
        assert_eq!(
            ngenrs_net_http_set_cert_path(std::ptr::null()),
            DynrsStatus::Ok
        );
        assert!(matches!(config_snapshot().0, CertConfig::Disabled));
    }

    #[test]
    fn proxy_config_round_trips_through_the_c_abi() {
        let _guard = lock_config();

        let host = CString::new("127.0.0.1").unwrap();
        let username = CString::new("user").unwrap();
        let password = CString::new("pwd").unwrap();
        let proxy = NgenrsHttpProxyConfig {
            host: host.as_ptr(),
            port: 3128,
            username: username.as_ptr(),
            password: password.as_ptr(),
        };

        assert_eq!(ngenrs_net_http_set_proxy(&proxy), DynrsStatus::Ok);
        let (_, proxy, _) = config_snapshot();
        let proxy = proxy.expect("proxy should be set");
        assert_eq!(proxy.host, "127.0.0.1");
        assert_eq!(proxy.port, 3128);
        assert_eq!(proxy.username, "user");
        assert_eq!(proxy.password, "pwd");

        // A null pointer clears the proxy, like DynXX does.
        assert_eq!(ngenrs_net_http_set_proxy(std::ptr::null()), DynrsStatus::Ok);
        let (_, proxy, _) = config_snapshot();
        assert!(proxy.is_none());

        // A port that does not fit the field is refused rather than silently becoming "no port".
        let wide = NgenrsHttpProxyConfig {
            host: host.as_ptr(),
            port: usize::from(u16::MAX) + 1,
            username: username.as_ptr(),
            password: password.as_ptr(),
        };
        assert_eq!(
            ngenrs_net_http_set_proxy(&wide),
            DynrsStatus::InvalidArgument
        );
        let (_, proxy, _) = config_snapshot();
        assert!(proxy.is_none(), "a refused proxy is not installed");
    }

    #[test]
    fn dns_configs_round_trip_through_the_c_abi() {
        let _guard = lock_config();

        let host = CString::new("pinned.test").unwrap();
        let address = CString::new("10.9.8.7").unwrap();
        let broken = CString::new("not-an-ip").unwrap();
        let good = NgenrsHttpDnsConfig {
            host: host.as_ptr(),
            port: 8443,
            address: address.as_ptr(),
        };

        assert_eq!(ngenrs_net_http_set_dns_configs(&good, 1), DynrsStatus::Ok);
        let (_, _, dns) = config_snapshot();
        assert_eq!(dns.len(), 1);
        assert_eq!(dns[0].host, "pinned.test");
        assert_eq!(dns[0].port, 8443);
        assert_eq!(dns[0].address, "10.9.8.7");

        // An entry with a null host, and one whose address no resolver could answer with, are both
        // refused: dropping them quietly is what let a caller believe a pin was in effect when it
        // was not.
        let null_host = NgenrsHttpDnsConfig {
            host: std::ptr::null(),
            port: 8443,
            address: address.as_ptr(),
        };
        assert_eq!(
            ngenrs_net_http_set_dns_configs(&null_host, 1),
            DynrsStatus::InvalidArgument
        );
        let unparseable = NgenrsHttpDnsConfig {
            host: host.as_ptr(),
            port: 0,
            address: broken.as_ptr(),
        };
        assert_eq!(
            ngenrs_net_http_set_dns_configs(&unparseable, 1),
            DynrsStatus::InvalidArgument
        );

        // Neither refusal disturbed the override that was already installed.
        let (_, _, dns) = config_snapshot();
        assert_eq!(dns.len(), 1);
        assert_eq!(dns[0].host, "pinned.test");

        // A zero count clears the overrides.
        assert_eq!(
            ngenrs_net_http_set_dns_configs(std::ptr::null(), 0),
            DynrsStatus::Ok
        );
        let (_, _, dns) = config_snapshot();
        assert!(dns.is_empty());
    }
}
