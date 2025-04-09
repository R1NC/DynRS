use std::collections::HashMap;
use std::os::raw::{c_char, c_void};
use std::path::Path;
use std::slice;
use crate::c::util::{cstr_to_rust, rust_to_cstr, rust_map_from_c_arrays, rust_map_to_c_arrays, ngenrs_free_ptr, box_into_raw_new};
use crate::core::net::{HttpClient, HttpResponse};
use once_cell::sync::Lazy;
use tokio::runtime::Runtime;

static RUNTIME: Lazy<Runtime> = Lazy::new(|| {
    Runtime::new().expect("Failed to create Tokio runtime")
});

#[unsafe(no_mangle)]
pub extern "C"
fn ngenrs_http_client_init(ca_cert_path: *const c_char) -> *mut c_void {
    let ca_path = if !ca_cert_path.is_null() {
        let path_str = cstr_to_rust(ca_cert_path).unwrap();
        Some(std::path::Path::new(path_str))
    } else {
        None
    };

    box_into_raw_new(
        HttpClient::new(ca_path).expect("Failed to create HTTP client")
    ) as *mut c_void
}

#[unsafe(no_mangle)]
pub extern "C" 
fn ngenrs_http_client_release(client: *mut c_void) {
    ngenrs_free_ptr(client)
}

#[unsafe(no_mangle)]
pub extern "C" 
fn ngenrs_http_get(
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

    let result = RUNTIME.block_on(async {
        client.get(&url, headers, body).await
    });

    match result {
        Ok(resp) => box_into_raw_new(resp) as *mut c_void,
        Err(_) => std::ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub extern "C" 
fn ngenrs_http_post(
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

    let result = RUNTIME.block_on(async {
        client.post(&url, headers, body, json_map).await
    });

    match result {
        Ok(resp) => box_into_raw_new(resp) as *mut c_void,
        Err(_) => std::ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub extern "C" 
fn ngenrs_http_download(
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

    let result = RUNTIME.block_on(async {
        client.download(&url, headers, output_path).await
    });

    match result {
        Ok(resp) => box_into_raw_new(resp) as *mut c_void,
        Err(_) => std::ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub extern "C" 
fn ngenrs_http_upload(
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

    let result = RUNTIME.block_on(async {
        client.upload(&url, headers, parts).await
    });

    match result {
        Ok(resp) => box_into_raw_new(resp) as *mut c_void,
        Err(_) => std::ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub extern "C"
fn ngenrs_http_parse_rsp_status(rsp_ptr: *mut c_void) -> i32 {
    if rsp_ptr.is_null() {
        return -1;
    }
    let rsp = unsafe { &*(rsp_ptr as *const HttpResponse) };
    rsp.status.as_u16() as i32
}

#[unsafe(no_mangle)]
pub extern "C"
fn ngenrs_http_parse_rsp_headers(
    rsp_ptr: *mut c_void,
    keys: *mut *mut c_char,
    values: *mut *mut c_char,
    count: *mut usize
) {
    if rsp_ptr.is_null() {
        return;
    }
    let rsp = unsafe { &*(rsp_ptr as *const HttpResponse) };
    let headers_map: HashMap<String, String> = rsp.headers.iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();

    unsafe { rust_map_to_c_arrays(&headers_map, keys, values, count) };
}

#[unsafe(no_mangle)]
pub extern "C" 
fn ngenrs_http_parse_rsp_body(rsp_ptr: *mut c_void) -> *mut c_char {
    if rsp_ptr.is_null() {
        return std::ptr::null_mut();
    }
    let rsp = unsafe { &*(rsp_ptr as *const HttpResponse) };
    match &rsp.body {
        Some(body) => rust_to_cstr(body.to_string()),
        None => std::ptr::null_mut(),
    }
}
