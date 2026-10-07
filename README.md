# DynRS

A cross-platform framework based on Rust, supporting biz dev via Lua & JS.

> :point_right: The modern C++ counterpart: [DynXX][1].

[<img src="https://img.shields.io/codecov/c/github/R1NC/DynRS/main?logo=codecov&label=Codecov"/>][11]
[![API Docs & Test Reports](https://img.shields.io/badge/API_Docs-Test_Reports-blue?logo=readthedocs)][4]
[![zread](https://img.shields.io/badge/Ask_Zread-_.svg?style=flat&color=00b0aa&labelColor=000000&logo=data%3Aimage%2Fsvg%2Bxml%3Bbase64%2CPHN2ZyB3aWR0aD0iMTYiIGhlaWdodD0iMTYiIHZpZXdCb3g9IjAgMCAxNiAxNiIgZmlsbD0ibm9uZSIgeG1sbnM9Imh0dHA6Ly93d3cudzMub3JnLzIwMDAvc3ZnIj4KPHBhdGggZD0iTTQuOTYxNTYgMS42MDAxSDIuMjQxNTZDMS44ODgxIDEuNjAwMSAxLjYwMTU2IDEuODg2NjQgMS42MDE1NiAyLjI0MDFWNC45NjAxQzEuNjAxNTYgNS4zMTM1NiAxLjg4ODEgNS42MDAxIDIuMjQxNTYgNS42MDAxSDQuOTYxNTZDNS4zMTUwMiA1LjYwMDEgNS42MDE1NiA1LjMxMzU2IDUuNjAxNTYgNC45NjAxVjIuMjQwMUM1LjYwMTU2IDEuODg2NjQgNS4zMTUwMiAxLjYwMDEgNC45NjE1NiAxLjYwMDFaIiBmaWxsPSIjZmZmIi8%2BCjxwYXRoIGQ9Ik00Ljk2MTU2IDEwLjM5OTlIMi4yNDE1NkMxLjg4ODEgMTAuMzk5OSAxLjYwMTU2IDEwLjY4NjQgMS42MDE1NiAxMS4wMzk5VjEzLjc1OTlDMS42MDE1NiAxNC4xMTM0IDEuODg4MSAxNC4zOTk5IDIuMjQxNTYgMTQuMzk5OUg0Ljk2MTU2QzUuMzE1MDIgMTQuMzk5OSA1LjYwMTU2IDE0LjExMzQgNS42MDE1NiAxMy43NTk5VjExLjAzOTlDNS42MDE1NiAxMC42ODY0IDUuMzE1MDIgMTAuMzk5OSA0Ljk2MTU2IDEwLjM5OTlaIiBmaWxsPSIjZmZmIi8%2BCjxwYXRoIGQ9Ik0xMy43NTg0IDEuNjAwMUgxMS4wMzg0QzEwLjY4NSAxLjYwMDEgMTAuMzk4NCAxLjg4NjY0IDEwLjM5ODQgMi4yNDAxVjQuOTYwMUMxMC4zOTg0IDUuMzEzNTYgMTAuNjg1IDUuNjAwMSAxMS4wMzg0IDUuNjAwMUgxMy43NTg0QzE0LjExMTkgNS42MDAxIDE0LjM5ODQgNS4zMTM1NiAxNC4zOTg0IDQuOTYwMVYyLjI0MDFDMTQuMzk4NCAxLjg4NjY0IDE0LjExMTkgMS42MDAxIDEzLjc1ODQgMS42MDAxWiIgZmlsbD0iI2ZmZiIvPgo8cGF0aCBkPSJNNCAxMkwxMiA0TDQgMTJaIiBmaWxsPSIjZmZmIi8%2BCjxwYXRoIGQ9Ik00IDEyTDEyIDQiIHN0cm9rZT0iI2ZmZiIgc3Ryb2tlLXdpZHRoPSIxLjUiIHN0cm9rZS1saW5lY2FwPSJyb3VuZCIvPgo8L3N2Zz4K&logoColor=ffffff)][2]  
[![Windows](https://img.shields.io/github/actions/workflow/status/R1NC/DynRS/CI-Windows-Win.yml?branch=main&logo=github&label=Windows)][3]
[![Linux](https://img.shields.io/github/actions/workflow/status/R1NC/DynRS/CI-Linux-Ubuntu.yml?branch=main&logo=github&label=Linux)][5]
[![macOS](https://img.shields.io/github/actions/workflow/status/R1NC/DynRS/CI-macOS-Mac.yml?branch=main&logo=github&label=macOS)][6]
[![iOS](https://img.shields.io/github/actions/workflow/status/R1NC/DynRS/CI-iOS-Mac.yml?branch=main&logo=github&label=iOS)][7]
[![Android](https://img.shields.io/github/actions/workflow/status/R1NC/DynRS/CI-Android-Ubuntu.yml?branch=main&logo=github&label=Android)][8]
[![OHOS](https://img.shields.io/github/actions/workflow/status/R1NC/DynRS/CI-OHOS-Ubuntu.yml?branch=main&logo=github&label=OHOS)][9]
[![WASM](https://img.shields.io/github/actions/workflow/status/R1NC/DynRS/CI-WASM-Ubuntu.yml?branch=main&logo=github&label=WASM)][10]

## :clipboard: Progress

| Module | Core | C ABI | Unit tests | Compared with DynXX |
| :-- | :--: | :--: | :--: | :-- |
| Crypto | :heavy_check_mark: | :heavy_check_mark: | :heavy_check_mark: | <ul><li>RSA encryption takes PKCS#1 and OAEP, the only paddings OpenSSL 3.x accepts</li><li>DynXX answers empty for the other padding ids too</li></ul> |
| Network | :heavy_check_mark: | :heavy_check_mark: | :heavy_check_mark: | <ul><li>`get` / `post` / `download` / `upload` instead of one `request`</li><li>CA path, proxy and DNS overrides included</li></ul> |
| SQLite | :heavy_check_mark: | :heavy_check_mark: | :heavy_check_mark: | |
| Key-Value | :heavy_check_mark: | :heavy_check_mark: | :heavy_check_mark: | <ul><li>one store key can hold one value per type, where DynXX's MMKV keeps a single typed value per key</li><li>DynXX's 256 byte key limit is not enforced</li></ul> |
| Zip | :heavy_check_mark: | :heavy_check_mark: | :heavy_check_mark: | <ul><li>one-shot `compress` / `decompress` only</li><li>DynXX's streaming `zip_init` / `input` / `process_do` API, its `FILE *` variants and the compression modes are not ported</li></ul> |
| Lua | :heavy_check_mark: | :heavy_check_mark: | :heavy_check_mark: | `addTimer` / `pollTimers` / `removeTimer` are a DynRS addition, DynXX has no timer API |
| JS (QuickJS) | :heavy_check_mark: | :heavy_check_mark: | :heavy_check_mark: | same three timer functions as the Lua bridge |
| Platform bridges (JNI / ArkTS / …) | | | | Not started |

* :heavy_check_mark: : Done;
* :x: : To do;

> **Unfixed advisory**: RSA decryption is not constant time, the `rsa` crate has no patch for the Marvin timing sidechannel (`RUSTSEC-2023-0071`). That advisory does not cover the OpenSSL RSA DynXX uses.

## :hammer_and_wrench: Build

* Rust 1.90+ (edition 2024). The floor is set by the dependency graph, not by the edition.
* `libclang` for `bindgen` (via `libquickjs-ng-sys`); set `LIBCLANG_PATH` when it is not on `PATH`.
* A C toolchain for the vendored QuickJS, Lua and SQLite.

```bash
cargo build                                 # the static library, plus the qjsc tool
cargo test
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo xtask build --target wasm --release   # also: android, ohos, ios (each needs its SDK)
```

## :test_tube: Tests

* `src/core/*` — behaviour of the portable layer, mirroring DynXX's gtest coverage.
* `src/c/*` — ABI contract tests only: null arguments, empty results, ownership.

## :left_right_arrow: The C ABI

Every exported function has the same shape: it returns a status, writes its results through
out-parameters, and never lets a panic reach the caller. A panic that escaped one of them would
abort the host process, so it is caught at the boundary and reported as `Panicked` instead.

### How a call reports what happened

The status separates answers that used to be one value — `false`, `0`, a null pointer — and the
out-parameters carry everything else:

| Status | Meaning |
| :-- | :-- |
| `Ok` (0) | The call did what it says. |
| `Empty` (1) | There was nothing to report, and that is not an error: no value under this key, no row is current, no body in this response. Kept apart from `Ok` so that a zero is never mistaken for data. |
| `InvalidArgument` (2) | The input was rejected: a null string, text that is not UTF-8, an empty key, a column the query did not select, a port that does not fit. |
| `InvalidHandle` (3) | The handle is null, or cannot answer yet — a result set before `ngenrs_db_next_row`. |
| `Failed` (4) | The operation failed: the network, the store, the script engine. |
| `Panicked` (5) | The body panicked: the host can tell "the library broke" from "my call was wrong". |

An out-parameter is left untouched when the call is refused, so read it only after `Ok`, `Empty`,
or the specific status you expect. A failure that has something to say beyond the status — a load
error, an unreadable response body — writes it to an `err_out` C string that the caller releases
with `ngenrs_free_cstr`.

### Memory ownership at the C ABI

Which release function a result needs is decided by *which out-parameter* it arrived in, not by the
call that returned it:

| Result arrived as | Release with |
| :-- | :-- |
| A byte buffer plus a length: `out` / `len_out` on `ngenrs_crypto_*`, `ngenrs_db_get_string`, `ngenrs_kv_read_string`, `ngenrs_qjs_call_function`, `ngenrs_lua_call_function`, `ngenrs_http_parse_rsp_body`, `ngenrs_z_compress` / `_decompress` | `ngenrs_free_bytes(ptr)` |
| A `*mut c_char`: `ngenrs_crypto_rsa_gen_key`'s `out`, any `err_out`, the entries of `ngenrs_http_parse_rsp_headers` | `ngenrs_free_cstr(ptr)` |
| A key list: `ngenrs_kv_all_keys`'s `out` | `ngenrs_kv_free_keys(keys)` |
| A handle: `ngenrs_*_open` / `_init` / `_query` | the matching `ngenrs_*_close` / `_release` / `_free_*` |
| An HTTP response: `ngenrs_http_get` / `_post` / `_download` / `_upload` | `ngenrs_http_release_rsp(rsp)` |

### Data is bytes, not C strings

Everything that hands back *data* returns a buffer plus its length through an out-parameter. A value
that is present but empty is a real allocation of length zero; "there is no value here" is
`DynrsStatus::Empty` with no buffer written at all.

The distinction matters because the data is not always text the library produced: a script may
return a string with a NUL byte in it, a text column may hold one, and an HTTP body is arbitrary
bytes. Report those as C strings and they are silently truncated to nothing, indistinguishable from
"no value". Only strings this library writes itself — error messages, PEM text, header names and
values — come back as C strings, and none of those can contain a NUL.

`ngenrs_kv_all_keys` follows the same rule for its keys: each key is a buffer with a length, because
a store key is a Rust `&str` that may itself contain a NUL, and a C string could not carry one.


[1]: https://github.com/R1NC/DynXX
[2]: https://zread.ai/R1NC/DynRS
[3]: https://github.com/R1NC/DynRS/actions/workflows/CI-Windows-Win.yml
[4]: https://R1NC.github.io/DynRS/
[5]: https://github.com/R1NC/DynRS/actions/workflows/CI-Linux-Ubuntu.yml
[6]: https://github.com/R1NC/DynRS/actions/workflows/CI-macOS-Mac.yml
[7]: https://github.com/R1NC/DynRS/actions/workflows/CI-iOS-Mac.yml
[8]: https://github.com/R1NC/DynRS/actions/workflows/CI-Android-Ubuntu.yml
[9]: https://github.com/R1NC/DynRS/actions/workflows/CI-OHOS-Ubuntu.yml
[10]: https://github.com/R1NC/DynRS/actions/workflows/CI-WASM-Ubuntu.yml
[11]: https://app.codecov.io/gh/R1NC/DynRS
