# DynRS

[![windows](https://img.shields.io/github/actions/workflow/status/R1NC/DynRS/CI-Windows-Win.yml?branch=main&label=windows-CI)](https://github.com/R1NC/DynRS/actions/workflows/CI-Windows-Win.yml)
[![linux](https://img.shields.io/github/actions/workflow/status/R1NC/DynRS/CI-Linux-Ubuntu.yml?branch=main&label=linux-CI)](https://github.com/R1NC/DynRS/actions/workflows/CI-Linux-Ubuntu.yml)
[![macos](https://img.shields.io/github/actions/workflow/status/R1NC/DynRS/CI-macOS-Mac.yml?branch=main&label=macos-CI)](https://github.com/R1NC/DynRS/actions/workflows/CI-macOS-Mac.yml)

A cross-platform framework based on Rust, supporting biz dev via Lua & JS.

> :point_right: The modern C++ version: [DynXX](https://github.com/R1NC/DynXX).

## :classical_building: Architecture

| Path | Contents |
| :-- | :-- |
| `src/core/*` | The portable logic: `crypto`, `db` (SQLite), `kv` (redb), `net`, `zip`, `lua`, `qjs`. |
| `src/c/*` | The hand-written C ABI (`ngenrs_*` symbols) that the platform bridges (JNI / ArkTS / …) call. |
| `src/bin/qjsc.rs` | QuickJS bytecode compiler. |

The crate builds as a `staticlib` plus a `cdylib`, so one code base serves every host.

## :clipboard: Status

| Module | Core | C ABI | Unit tests | Compared with DynXX |
| :-- | :--: | :--: | :--: | :-- |
| Crypto | :heavy_check_mark: | :heavy_check_mark: | :heavy_check_mark: | RSA covers PKCS#1 and OAEP only |
| Network | :heavy_check_mark: | :heavy_check_mark: | :heavy_check_mark: | `get` / `post` / `download` / `upload` instead of one `request`; CA path, proxy and DNS overrides included |
| SQLite | :heavy_check_mark: | :heavy_check_mark: | :heavy_check_mark: | |
| Key-Value | :heavy_check_mark: | :heavy_check_mark: | :heavy_check_mark: | one store key can hold one value per type, where DynXX's MMKV keeps a single typed value per key; DynXX's 256 byte key limit is not enforced |
| Zip | :heavy_check_mark: | :heavy_check_mark: | :heavy_check_mark: | one-shot `compress` / `decompress` only; DynXX's streaming `zip_init` / `input` / `process_do` API, its `FILE *` variants and the compression modes are not ported |
| Lua | :heavy_check_mark: | :heavy_check_mark: | :x: | |
| JS (QuickJS) | :heavy_check_mark: | :heavy_check_mark: | :x: | |
| Platform bridges (JNI / ArkTS / …) | | | | Not started |

* :heavy_check_mark: : Done;
* :x: : To do;

## :hammer_and_wrench: Build

Prerequisites:

* Rust stable with edition 2024 support (1.85+). The code deliberately stays on 1.85 APIs.
* `libclang`, required by `bindgen` through `libquickjs-ng-sys`. Point `LIBCLANG_PATH` at the
  directory holding `libclang.dll` / `libclang.so` / `libclang.dylib`; a normal LLVM install already
  puts it on `PATH` on Windows.
* A C toolchain for the vendored bundles: QuickJS (`libquickjs-ng-sys`), Lua (`mlua`, feature
  `vendored`) and SQLite (`rusqlite`, feature `bundled`).

```bash
cargo build                                 # staticlib + cdylib, plus the qjsc tool
cargo test                                  # unit tests
cargo fmt --all --check                     # formatting gate
cargo clippy --all-targets -- -D warnings   # lint gate
```

`Cargo.lock` is committed and CI builds with `--locked`.

Note: `aarch64-apple-darwin` must not carry `-C link-arg=-static`: macOS cannot link executables
statically (`ld: library 'crt0.o' not found`), while build scripts, proc macros, the cdylib and
`qjsc` all link. A `staticlib` needs no linker flags. Clearing it afterwards with `cargo --config`
does not work either, because cargo joins array values instead of replacing them.

## :test_tube: Tests

* `src/core/*` — the behaviour of the portable layer, mirroring what DynXX covers with gtest.
* `src/c/*` — contract tests at the ABI seam only: null arguments, empty results, ownership and the
  release helpers.
* Platform bridges (JNI / ArkTS / …) have no automated tests, the same as DynXX.

### Memory ownership at the C ABI

| Returned value | Release with |
| :-- | :-- |
| Byte buffers: `ngenrs_crypto_rand`, AES / RSA / hash / base64 outputs | `ngenrs_free_bytes(ptr, len)` |
| C strings: `ngenrs_crypto_rsa_gen_key`, `ngenrs_*_read_string`, `ngenrs_db_get_string`, `ngenrs_http_parse_rsp_body` | `ngenrs_free_cstr(ptr)` |
| Handles: `ngenrs_*_open` / `_init` / `_query` | the matching `ngenrs_*_close` / `_release` / `_free_*` |

## :rocket: CI

Only the host workflows are badged above; `Common.yml` reports through the checks list. Because
every host is its own workflow, a failure on one never cancels or hides another:

| Workflow | Contents |
| :-- | :-- |
| `Common.yml` | `cargo fmt --all --check` and `cargo clippy --all-targets -- -D warnings`; not tied to a platform |
| `CI-Windows-Win.yml` | build and test on `windows-latest` |
| `CI-Linux-Ubuntu.yml` | the same on `ubuntu-latest` |
| `CI-macOS-Mac.yml` | the same on `macos-latest`, with the Apple `-static` rustflags cleared |

Each workflow triggers on pushes to `main` and on pull requests, filtered by `paths` to the sources,
the manifests and the build configuration (plus its own file), and each can also be started by hand
through `workflow_dispatch`. Every build uses `--locked`. Clippy runs on a single host because the
source has no platform conditional compilation, so one host covers all of it.
