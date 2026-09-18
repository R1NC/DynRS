# DynRS

A cross-platform framework based on Rust, supporting biz dev via Lua & JS.

> :point_right: The modern C++ version: [DynXX][1].

[![API Docs & Test Reports](https://img.shields.io/badge/API_Docs_%26_Test_Reports-gray?logo=github)][4]
[![zread](https://img.shields.io/badge/Ask_Zread-_.svg?style=flat&color=00b0aa&labelColor=000000&logo=data%3Aimage%2Fsvg%2Bxml%3Bbase64%2CPHN2ZyB3aWR0aD0iMTYiIGhlaWdodD0iMTYiIHZpZXdCb3g9IjAgMCAxNiAxNiIgZmlsbD0ibm9uZSIgeG1sbnM9Imh0dHA6Ly93d3cudzMub3JnLzIwMDAvc3ZnIj4KPHBhdGggZD0iTTQuOTYxNTYgMS42MDAxSDIuMjQxNTZDMS44ODgxIDEuNjAwMSAxLjYwMTU2IDEuODg2NjQgMS42MDE1NiAyLjI0MDFWNC45NjAxQzEuNjAxNTYgNS4zMTM1NiAxLjg4ODEgNS42MDAxIDIuMjQxNTYgNS42MDAxSDQuOTYxNTZDNS4zMTUwMiA1LjYwMDEgNS42MDE1NiA1LjMxMzU2IDUuNjAxNTYgNC45NjAxVjIuMjQwMUM1LjYwMTU2IDEuODg2NjQgNS4zMTUwMiAxLjYwMDEgNC45NjE1NiAxLjYwMDFaIiBmaWxsPSIjZmZmIi8%2BCjxwYXRoIGQ9Ik00Ljk2MTU2IDEwLjM5OTlIMi4yNDE1NkMxLjg4ODEgMTAuMzk5OSAxLjYwMTU2IDEwLjY4NjQgMS42MDE1NiAxMS4wMzk5VjEzLjc1OTlDMS42MDE1NiAxNC4xMTM0IDEuODg4MSAxNC4zOTk5IDIuMjQxNTYgMTQuMzk5OUg0Ljk2MTU2QzUuMzE1MDIgMTQuMzk5OSA1LjYwMTU2IDE0LjExMzQgNS42MDE1NiAxMy43NTk5VjExLjAzOTlDNS42MDE1NiAxMC42ODY0IDUuMzE1MDIgMTAuMzk5OSA0Ljk2MTU2IDEwLjM5OTlaIiBmaWxsPSIjZmZmIi8%2BCjxwYXRoIGQ9Ik0xMy43NTg0IDEuNjAwMUgxMS4wMzg0QzEwLjY4NSAxLjYwMDEgMTAuMzk4NCAxLjg4NjY0IDEwLjM5ODQgMi4yNDAxVjQuOTYwMUMxMC4zOTg0IDUuMzEzNTYgMTAuNjg1IDUuNjAwMSAxMS4wMzg0IDUuNjAwMUgxMy43NTg0QzE0LjExMTkgNS42MDAxIDE0LjM5ODQgNS4zMTM1NiAxNC4zOTg0IDQuOTYwMVYyLjI0MDFDMTQuMzk4NCAxLjg4NjY0IDE0LjExMTkgMS42MDAxIDEzLjc1ODQgMS42MDAxWiIgZmlsbD0iI2ZmZiIvPgo8cGF0aCBkPSJNNCAxMkwxMiA0TDQgMTJaIiBmaWxsPSIjZmZmIi8%2BCjxwYXRoIGQ9Ik00IDEyTDEyIDQiIHN0cm9rZT0iI2ZmZiIgc3Ryb2tlLXdpZHRoPSIxLjUiIHN0cm9rZS1saW5lY2FwPSJyb3VuZCIvPgo8L3N2Zz4K&logoColor=ffffff)][2]  
[![Windows](https://img.shields.io/github/actions/workflow/status/R1NC/DynRS/CI-Windows-Win.yml?branch=main&label=Windows)][3]
[![Linux](https://img.shields.io/github/actions/workflow/status/R1NC/DynRS/CI-Linux-Ubuntu.yml?branch=main&label=Linux)][5]
[![macOS](https://img.shields.io/github/actions/workflow/status/R1NC/DynRS/CI-macOS-Mac.yml?branch=main&label=macOS)][6]
[![iOS](https://img.shields.io/github/actions/workflow/status/R1NC/DynRS/CI-iOS-Mac.yml?branch=main&label=iOS)][7]
[![Android](https://img.shields.io/github/actions/workflow/status/R1NC/DynRS/CI-Android-Ubuntu.yml?branch=main&label=Android)][8]
[![OHOS](https://img.shields.io/github/actions/workflow/status/R1NC/DynRS/CI-OHOS-Ubuntu.yml?branch=main&label=OHOS)][9]
[![WASM](https://img.shields.io/github/actions/workflow/status/R1NC/DynRS/CI-WASM-Ubuntu.yml?branch=main&label=WASM)][10]

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

### Memory ownership at the C ABI

| Returned value | Release with |
| :-- | :-- |
| Byte buffers: `ngenrs_crypto_rand`, AES / RSA / hash / base64 outputs | `ngenrs_free_bytes(ptr, len)` |
| C strings: `ngenrs_crypto_rsa_gen_key`, `ngenrs_*_read_string`, `ngenrs_db_get_string`, `ngenrs_http_parse_rsp_body` | `ngenrs_free_cstr(ptr)` |
| Handles: `ngenrs_*_open` / `_init` / `_query` | the matching `ngenrs_*_close` / `_release` / `_free_*` |
| HTTP responses: `ngenrs_http_get` / `_post` / `_download` / `_upload` | `ngenrs_http_release_rsp(rsp)` |


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
