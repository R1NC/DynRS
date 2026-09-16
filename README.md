# DynRS

[![windows](https://img.shields.io/github/actions/workflow/status/R1NC/DynRS/CI-Windows-Win.yml?branch=main&label=windows-CI)](https://github.com/R1NC/DynRS/actions/workflows/CI-Windows-Win.yml)
[![linux](https://img.shields.io/github/actions/workflow/status/R1NC/DynRS/CI-Linux-Ubuntu.yml?branch=main&label=linux-CI)](https://github.com/R1NC/DynRS/actions/workflows/CI-Linux-Ubuntu.yml)
[![macos](https://img.shields.io/github/actions/workflow/status/R1NC/DynRS/CI-macOS-Mac.yml?branch=main&label=macos-CI)](https://github.com/R1NC/DynRS/actions/workflows/CI-macOS-Mac.yml)

A cross-platform framework based on Rust, supporting biz dev via Lua & JS.

> :point_right: The modern C++ version: [DynXX](https://github.com/R1NC/DynXX).

## :clipboard: Status

| Module | Core | C ABI | Unit tests | Compared with DynXX |
| :-- | :--: | :--: | :--: | :-- |
| Crypto | :heavy_check_mark: | :heavy_check_mark: | :heavy_check_mark: | RSA encryption takes PKCS#1 and OAEP, the only paddings OpenSSL 3.x accepts; DynXX answers empty for the other padding ids too |
| Network | :heavy_check_mark: | :heavy_check_mark: | :heavy_check_mark: | `get` / `post` / `download` / `upload` instead of one `request`; CA path, proxy and DNS overrides included |
| SQLite | :heavy_check_mark: | :heavy_check_mark: | :heavy_check_mark: | |
| Key-Value | :heavy_check_mark: | :heavy_check_mark: | :heavy_check_mark: | one store key can hold one value per type, where DynXX's MMKV keeps a single typed value per key; DynXX's 256 byte key limit is not enforced |
| Zip | :heavy_check_mark: | :heavy_check_mark: | :heavy_check_mark: | one-shot `compress` / `decompress` only; DynXX's streaming `zip_init` / `input` / `process_do` API, its `FILE *` variants and the compression modes are not ported |
| Lua | :heavy_check_mark: | :heavy_check_mark: | :heavy_check_mark: | `addTimer` / `pollTimers` / `removeTimer` are a DynRS addition, DynXX has no timer API |
| JS (QuickJS) | :heavy_check_mark: | :heavy_check_mark: | :heavy_check_mark: | same three timer functions as the Lua bridge |
| Platform bridges (JNI / ArkTS / …) | | | | Not started |

* :heavy_check_mark: : Done;
* :x: : To do;

## :hammer_and_wrench: Build

* Rust with edition 2024 support (1.85+).
* `libclang` for `bindgen` (via `libquickjs-ng-sys`); set `LIBCLANG_PATH` when it is not on `PATH`.
* A C toolchain for the vendored QuickJS, Lua and SQLite.

```bash
cargo build                                 # staticlib + cdylib, plus the qjsc tool
cargo test
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
```

`Cargo.lock` is committed, so CI builds with `--locked`.

## :test_tube: Tests

* `src/core/*` — behaviour of the portable layer, mirroring DynXX's gtest coverage.
* `src/c/*` — ABI contract tests only: null arguments, empty results, ownership.
* Timers run on the host thread: `addTimer` schedules, `pollTimers` runs what is due, `removeTimer` drops one. A callback may schedule another timer.
* JNI / ArkTS bridges are not covered, the same as DynXX.

### Memory ownership at the C ABI

| Returned value | Release with |
| :-- | :-- |
| Byte buffers: `ngenrs_crypto_rand`, AES / RSA / hash / base64 outputs | `ngenrs_free_bytes(ptr, len)` |
| C strings: `ngenrs_crypto_rsa_gen_key`, `ngenrs_*_read_string`, `ngenrs_db_get_string`, `ngenrs_http_parse_rsp_body` | `ngenrs_free_cstr(ptr)` |
| Handles: `ngenrs_*_open` / `_init` / `_query` | the matching `ngenrs_*_close` / `_release` / `_free_*` |
