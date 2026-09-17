# DynRS

A cross-platform framework based on Rust, supporting biz dev via Lua & JS.

> :point_right: The modern C++ version: [DynXX](https://github.com/R1NC/DynXX).

[![API Docs & Test Reports](https://img.shields.io/badge/API_Docs_%26_Test_Reports-gray?logo=github)](https://R1NC.github.io/DynRS/)

[![Windows](https://img.shields.io/github/actions/workflow/status/R1NC/DynRS/CI-Windows-Win.yml?branch=main&label=Windows)](https://github.com/R1NC/DynRS/actions/workflows/CI-Windows-Win.yml)
[![Linux](https://img.shields.io/github/actions/workflow/status/R1NC/DynRS/CI-Linux-Ubuntu.yml?branch=main&label=Linux)](https://github.com/R1NC/DynRS/actions/workflows/CI-Linux-Ubuntu.yml)
[![macOS](https://img.shields.io/github/actions/workflow/status/R1NC/DynRS/CI-macOS-Mac.yml?branch=main&label=macOS)](https://github.com/R1NC/DynRS/actions/workflows/CI-macOS-Mac.yml)
[![iOS](https://img.shields.io/github/actions/workflow/status/R1NC/DynRS/CI-iOS-Mac.yml?branch=main&label=iOS)](https://github.com/R1NC/DynRS/actions/workflows/CI-iOS-Mac.yml)
[![Android](https://img.shields.io/github/actions/workflow/status/R1NC/DynRS/CI-Android-Ubuntu.yml?branch=main&label=Android)](https://github.com/R1NC/DynRS/actions/workflows/CI-Android-Ubuntu.yml)
[![OHOS](https://img.shields.io/github/actions/workflow/status/R1NC/DynRS/CI-OHOS-Ubuntu.yml?branch=main&label=OHOS)](https://github.com/R1NC/DynRS/actions/workflows/CI-OHOS-Ubuntu.yml)
[![WASM](https://img.shields.io/github/actions/workflow/status/R1NC/DynRS/CI-WASM-Ubuntu.yml?branch=main&label=WASM)](https://github.com/R1NC/DynRS/actions/workflows/CI-WASM-Ubuntu.yml)

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
