use crate::c::util::{
    box_into_raw_new, cstr_to_rust, ngenrs_free_ptr, rust_map_from_c_arrays, rust_map_to_c_arrays,
    rust_to_cstr,
};
use crate::core::net::{
    DnsConfig, HttpClient, HttpResponse, set_cert_path, set_dns_configs, set_proxy,
};
use std::collections::HashMap;
use std::os::raw::{c_char, c_void};
use std::path::Path;
use std::slice;
use std::sync::LazyLock;
use tokio::runtime::Runtime;

static RUNTIME: LazyLock<Runtime> =
    LazyLock::new(|| Runtime::new().expect("Failed to create Tokio runtime"));

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_http_client_init(ca_cert_path: *const c_char) -> *mut c_void {
    let ca_path = if !ca_cert_path.is_null() {
        let path_str = cstr_to_rust(ca_cert_path).unwrap();
        Some(std::path::Path::new(path_str))
    } else {
        None
    };

    box_into_raw_new(HttpClient::new(ca_path).expect("Failed to create HTTP client")) as *mut c_void
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_http_client_release(client: *mut c_void) {
    if !client.is_null() {
        unsafe { drop(Box::from_raw(client as *mut HttpClient)) };
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

/// Mirrors `dynxx_net_http_set_cert_path`: a null or empty path disables peer
/// verification, any other value is used as the CA bundle path.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_net_http_set_cert_path(path: *const c_char) {
    let path = if path.is_null() {
        None
    } else {
        cstr_to_rust(path)
    };
    set_cert_path(path);
}

/// Mirrors `dynxx_net_http_set_proxy`: a null pointer or an empty host clears the
/// proxy. `port` is a `size_t` like DynXX's; values above `u16::MAX` fall back to 0,
/// which means "do not append a port".
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_net_http_set_proxy(proxy: *const NgenrsHttpProxyConfig) {
    let Some(proxy) = (unsafe { proxy.as_ref() }) else {
        set_proxy(None, 0, "", "");
        return;
    };

    set_proxy(
        cstr_to_rust(proxy.host),
        u16::try_from(proxy.port).unwrap_or(0),
        cstr_to_rust(proxy.username).unwrap_or_default(),
        cstr_to_rust(proxy.password).unwrap_or_default(),
    );
}

/// Mirrors `dynxx_net_http_set_dns_configs`: a null pointer or a zero count clears the
/// overrides, and entries with a null host or address are skipped, the same way DynXX
/// drops invalid configs.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_net_http_set_dns_configs(
    configs: *const NgenrsHttpDnsConfig,
    config_count: usize,
) {
    if configs.is_null() || config_count == 0 {
        set_dns_configs(&[]);
        return;
    }

    let configs = unsafe { slice::from_raw_parts(configs, config_count) };
    let configs = configs
        .iter()
        .filter_map(|config| {
            Some(DnsConfig {
                host: cstr_to_rust(config.host)?.to_string(),
                port: u16::try_from(config.port).unwrap_or(0),
                address: cstr_to_rust(config.address)?.to_string(),
            })
        })
        .collect::<Vec<_>>();

    set_dns_configs(&configs);
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_http_get(
    client: *const c_void,
    url: *const c_char,
    header_keys: *const *const c_char,
    header_values: *const *const c_char,
    headers_len: usize,
    body: *const c_char,
) -> *mut c_void {
    let client = unsafe { &*(client as *const HttpClient) };
    let url = cstr_to_rust(url).unwrap_or_default();
    let headers = unsafe { rust_map_from_c_arrays(header_keys, header_values, headers_len) };
    let body = if !body.is_null() {
        Some(cstr_to_rust(body).unwrap_or_default())
    } else {
        None
    };

    let result = RUNTIME.block_on(async { client.get(url, headers, body).await });

    match result {
        Ok(resp) => box_into_raw_new(resp) as *mut c_void,
        Err(_) => std::ptr::null_mut(),
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
) -> *mut c_void {
    let client = unsafe { &*(client as *const HttpClient) };
    let url = cstr_to_rust(url).unwrap_or_default();
    let headers = unsafe { rust_map_from_c_arrays(header_keys, header_values, headers_len) };
    let body = if !body.is_null() {
        Some(cstr_to_rust(body).unwrap_or_default())
    } else {
        None
    };
    let json_map = unsafe { rust_map_from_c_arrays(json_keys, json_values, json_len) };

    let result = RUNTIME.block_on(async { client.post(url, headers, body, json_map).await });

    match result {
        Ok(resp) => box_into_raw_new(resp) as *mut c_void,
        Err(_) => std::ptr::null_mut(),
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
) -> *mut c_void {
    let client = unsafe { &*(client as *const HttpClient) };
    let url = cstr_to_rust(url).unwrap_or_default();
    let headers = unsafe { rust_map_from_c_arrays(header_keys, header_values, headers_len) };
    let output_path = Path::new(cstr_to_rust(output_path).unwrap_or_default());

    let result = RUNTIME.block_on(async { client.download(url, headers, output_path).await });

    match result {
        Ok(resp) => box_into_raw_new(resp) as *mut c_void,
        Err(_) => std::ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_http_upload(
    client: *const c_void,
    url: *const c_char,
    header_keys: *const *const c_char,
    header_values: *const *const c_char,
    headers_len: usize,
    part_names: *const *const c_char,
    part_data: *const *const u8,
    part_data_lens: *const usize,
    part_mimes: *const *const c_char,
    part_filenames: *const *const c_char,
    parts_len: usize,
) -> *mut c_void {
    let client = unsafe { &*(client as *const HttpClient) };
    let url = cstr_to_rust(url).unwrap_or_default();
    let headers = unsafe { rust_map_from_c_arrays(header_keys, header_values, headers_len) };

    let mut parts = Vec::new();
    unsafe {
        let names = slice::from_raw_parts(part_names, parts_len);
        let datas = slice::from_raw_parts(part_data, parts_len);
        let data_lens = slice::from_raw_parts(part_data_lens, parts_len);
        let mimes = slice::from_raw_parts(part_mimes, parts_len);
        let filenames = slice::from_raw_parts(part_filenames, parts_len);

        for i in 0..parts_len {
            let name = cstr_to_rust(names[i]).unwrap_or_default().to_string();
            let data = slice::from_raw_parts(datas[i], data_lens[i]).to_vec();
            let mime = if !mimes[i].is_null() {
                Some(cstr_to_rust(mimes[i]).unwrap_or_default().to_string())
            } else {
                None
            };
            let filename = if !filenames[i].is_null() {
                Some(cstr_to_rust(filenames[i]).unwrap_or_default().to_string())
            } else {
                None
            };

            parts.push((name, data, mime, filename));
        }
    }

    let result = RUNTIME.block_on(async { client.upload(url, headers, parts).await });

    match result {
        Ok(resp) => box_into_raw_new(resp) as *mut c_void,
        Err(_) => std::ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_http_parse_rsp_status(rsp_ptr: *mut c_void) -> i32 {
    if rsp_ptr.is_null() {
        return -1;
    }
    let rsp = unsafe { &*(rsp_ptr as *const HttpResponse) };
    rsp.status.as_u16() as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_http_parse_rsp_headers(
    rsp_ptr: *mut c_void,
    keys: *mut *mut c_char,
    values: *mut *mut c_char,
    count: *mut usize,
) {
    if rsp_ptr.is_null() {
        return;
    }
    let rsp = unsafe { &*(rsp_ptr as *const HttpResponse) };
    let headers_map: HashMap<String, String> = rsp
        .headers
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();

    unsafe { rust_map_to_c_arrays(&headers_map, keys, values, count) };
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_http_parse_rsp_body(rsp_ptr: *mut c_void) -> *mut c_char {
    if rsp_ptr.is_null() {
        return std::ptr::null_mut();
    }
    let rsp = unsafe { &*(rsp_ptr as *const HttpResponse) };
    match &rsp.body {
        Some(body) => rust_to_cstr(body.to_string()),
        None => std::ptr::null_mut(),
    }
}

/// Releases a response of `ngenrs_http_get`, `ngenrs_http_post`, `ngenrs_http_download` or
/// `ngenrs_http_upload`. The `parse_rsp_*` readers only look at the response, so the caller has to
/// hand it back once it is done with it.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_http_release_rsp(rsp_ptr: *mut c_void) {
    ngenrs_free_ptr(rsp_ptr as *mut HttpResponse);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::c::util::ngenrs_free_cstr;
    use crate::core::net::{CONFIG_TEST_LOCK, config_snapshot};
    use crate::core::net_test_server::{Server, behind_an_environment_proxy, ok};
    use std::ffi::{CStr, CString};
    use std::sync::MutexGuard;

    fn lock_config() -> MutexGuard<'static, ()> {
        CONFIG_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// A client over a clean global config, with the lock that keeps it stable, or `None` when a
    /// proxy in the environment would keep the requests away from this machine anyway.
    fn client_and_clean_config() -> Option<(*mut c_void, MutexGuard<'static, ()>)> {
        if behind_an_environment_proxy() {
            return None;
        }

        let guard = lock_config();
        // A clean config, through the entry points a C caller would use as well.
        ngenrs_net_http_set_cert_path(std::ptr::null());
        ngenrs_net_http_set_proxy(std::ptr::null());
        ngenrs_net_http_set_dns_configs(std::ptr::null(), 0);

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

        let response = ngenrs_http_get(
            client,
            url.as_ptr(),
            keys.as_ptr(),
            values.as_ptr(),
            keys.len(),
            body.as_ptr(),
        );
        assert!(!response.is_null(), "the request went through");
        assert_eq!(ngenrs_http_parse_rsp_status(response), 200);

        let parsed = ngenrs_http_parse_rsp_body(response);
        assert_eq!(unsafe { CStr::from_ptr(parsed) }.to_str().unwrap(), "hello");
        ngenrs_free_cstr(parsed);

        // The headers are handed out as strings, one `ngenrs_free_cstr` each.
        let mut header_keys = [std::ptr::null_mut(); 8];
        let mut header_values = [std::ptr::null_mut(); 8];
        let mut header_count = 0;
        ngenrs_http_parse_rsp_headers(
            response,
            header_keys.as_mut_ptr(),
            header_values.as_mut_ptr(),
            &mut header_count,
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

        // A null pointer is accepted by every reader, and by the release.
        assert_eq!(ngenrs_http_parse_rsp_status(std::ptr::null_mut()), -1);
        assert!(ngenrs_http_parse_rsp_body(std::ptr::null_mut()).is_null());
        ngenrs_http_parse_rsp_headers(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
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

        let response = ngenrs_http_post(
            client,
            url.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            std::ptr::null(),
            json_keys.as_ptr(),
            json_values.as_ptr(),
            json_keys.len(),
        );
        assert!(!response.is_null(), "the request went through");
        assert_eq!(ngenrs_http_parse_rsp_status(response), 200);

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
        let response = ngenrs_http_post(
            client,
            url.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            body.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
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

        let response = ngenrs_http_download(
            client,
            url.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            output.as_ptr(),
        );
        assert!(!response.is_null(), "the request went through");
        assert_eq!(ngenrs_http_parse_rsp_status(response), 200);
        assert!(
            ngenrs_http_parse_rsp_body(response).is_null(),
            "the body of a download is the file, not a string in the response"
        );
        assert_eq!(
            std::fs::read_to_string(&path).expect("the file was written"),
            "hello"
        );
        std::fs::remove_file(&path).expect("the file is removed again");

        assert!(server.request(0).starts_with("GET /file"));
        ngenrs_http_release_rsp(response);
        ngenrs_http_client_release(client);
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

        let names = [first.as_ptr(), second.as_ptr()];
        let datas = [data_first.as_ptr(), data_second.as_ptr()];
        let lengths = [data_first.len(), data_second.len()];
        let mimes = [mime.as_ptr(), std::ptr::null()];
        let filenames = [filename.as_ptr(), std::ptr::null()];

        let response = ngenrs_http_upload(
            client,
            url.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            names.as_ptr(),
            datas.as_ptr(),
            lengths.as_ptr(),
            mimes.as_ptr(),
            filenames.as_ptr(),
            names.len(),
        );
        assert!(!response.is_null(), "the request went through");
        assert_eq!(ngenrs_http_parse_rsp_status(response), 200);

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

    /// A request that cannot be made at all answers with a null response.
    #[test]
    fn a_request_that_fails_gives_a_null_response() {
        let Some((client, _guard)) = client_and_clean_config() else {
            return;
        };
        // A URL reqwest refuses to parse is the one failure that needs no server and cannot hang:
        // the port of a listener that was just closed can be handed out again in the meantime.
        let url = CString::new("not a url").unwrap();

        let response = ngenrs_http_get(
            client,
            url.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            std::ptr::null(),
        );
        assert!(
            response.is_null(),
            "the request failed, so there is no response"
        );

        ngenrs_http_client_release(client);
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

        ngenrs_net_http_set_proxy(&proxy);
        let (_, proxy, _) = config_snapshot();
        let proxy = proxy.expect("proxy should be set");
        assert_eq!(proxy.host, "127.0.0.1");
        assert_eq!(proxy.port, 3128);
        assert_eq!(proxy.username, "user");
        assert_eq!(proxy.password, "pwd");

        // A null pointer clears the proxy, like DynXX does.
        ngenrs_net_http_set_proxy(std::ptr::null());
        let (_, proxy, _) = config_snapshot();
        assert!(proxy.is_none());
    }

    #[test]
    fn dns_configs_round_trip_through_the_c_abi() {
        let _guard = lock_config();

        let host = CString::new("pinned.test").unwrap();
        let address = CString::new("10.9.8.7").unwrap();
        let broken = CString::new("not-an-ip").unwrap();
        let configs = [
            NgenrsHttpDnsConfig {
                host: host.as_ptr(),
                port: 8443,
                address: address.as_ptr(),
            },
            // A null host is skipped, like DynXX does.
            NgenrsHttpDnsConfig {
                host: std::ptr::null(),
                port: 8443,
                address: address.as_ptr(),
            },
            // So is an address no resolver could answer with.
            NgenrsHttpDnsConfig {
                host: host.as_ptr(),
                port: 0,
                address: broken.as_ptr(),
            },
        ];

        ngenrs_net_http_set_dns_configs(configs.as_ptr(), configs.len());
        let (_, _, dns) = config_snapshot();
        assert_eq!(dns.len(), 1);
        assert_eq!(dns[0].host, "pinned.test");
        assert_eq!(dns[0].port, 8443);
        assert_eq!(dns[0].address, "10.9.8.7");

        // A zero count clears the overrides.
        ngenrs_net_http_set_dns_configs(std::ptr::null(), 0);
        let (_, _, dns) = config_snapshot();
        assert!(dns.is_empty());
    }
}
