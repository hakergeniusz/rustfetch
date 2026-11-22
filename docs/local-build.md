# Local-only (machine-tuned) build

`local` is a build profile plus a helper script for producing the fastest
rustfetch binary **for the machine it is built on**, without changing the
normal release build. It is not a distribution artifact and is not portable.

## Build

From the repository root:

```bash
scripts/build-local.sh
```

The script runs exactly:

```bash
RUSTFLAGS='-C target-cpu=native' cargo build --profile local --locked
```

and leaves the binary at `target/local/rustfetch` (cargo's natural directory
for a custom profile named `local`). It prints the binary size and, when
`hyperfine` is installed and a `target/release/rustfetch` exists, a quick
`local` vs `release` benchmark. Use `scripts/build-local.sh --no-bench` to skip
the benchmark, or `-h`/`--help` for the full option list.

Equivalent manual build:

```bash
RUSTFLAGS='-C target-cpu=native' cargo build --profile local --locked
./target/local/rustfetch --local-only
```

Build a matching reference binary with `cargo build --release --locked` first
if you want the comparison. Environment knobs:

- `RUSTFETCH_LOCAL_FLAGS` — replaces the default RUSTFLAGS (default
  `-C target-cpu=native`). If you override it and still want CPU-specific
  instructions, include `-C target-cpu=native` yourself.
- `RUSTFETCH_BENCH_ARGS` — argv passed identically to both benchmarked
  binaries (the script auto-detects `--local-only` when both binaries support
  it).
- `CARGO_TARGET_DIR` — honored by cargo and by the script's binary lookup.

## What each setting buys

`[profile.local]` in `Cargo.toml` (the `[profile.release]` block is untouched):

| Setting | Effect |
| --- | --- |
| `inherits = "release"` | Required by Cargo for custom profiles; starts from the release baseline. |
| `opt-level = 3` | Maximum LLVM optimization: aggressive inlining, vectorization, loop transforms. |
| `lto = "fat"` | Whole-program LTO across all crates, including `libc`; more inlining and dead-code removal than release's `thin`. Slowest to link. |
| `codegen-units = 1` | One LLVM codegen unit gives the optimizer a complete view of the crate (release uses 4). Trades parallel compile speed for code quality. |
| `panic = "abort"` | No unwinding tables or landing pads; smaller binary and slightly less work on every call. A panic terminates the process instead of unwinding. |
| `strip = "symbols"` | Removes the symbol table after linking; smaller on-disk binary and less loader work. |
| `debug = false` | No DWARF debug info; smaller binary. |
| `incremental = false` | No incremental compilation metadata; the output is a clean, fully optimized build and nothing is kept for faster rebuilds. |

`RUSTFLAGS='-C target-cpu=native'` (in the script, not the profile) lets LLVM
emit instructions for the exact CPU detected at build time — e.g. AVX2 on the
build host — instead of conservative baseline x86-64.

### The `--local-only` runtime flag

The local binary accepts one rustfetch-only flag: `--local-only`. It removes
the `publicip` and `weather` modules from the selected structure (both
`--structure` tokens and config `modules` entries) before module
preparation/collection, so the binary never waits on DNS or HTTP. It works in
both text and JSON output and leaves ordinary output unchanged when the flag
is absent. Version-string detection performed inside shared modules still
spawns subprocesses; only the two network modules are gated.

## Portability caveat

**The binary is NOT portable.** `-C target-cpu=native` bakes in instructions
from the build machine's CPU. Running it on a different CPU (or after a VM
migration / hardware change) can crash with `SIGILL`. Do not copy it to other
machines, ship it in packages, or use it as a CI artifact — build a fresh
binary on each target machine. The default `release` build stays portable.

`--local-only` only changes which modules run; it does not make the binary
portable.

## Benchmark results

Measured on the build host: Arch Linux, rustc/cargo 1.100.0, Intel Core
i5-12400F (12 threads), shared desktop under load. Both binaries came from the
same source tree; `release` was built with `cargo build --release --locked`
(default RUSTFLAGS), `local` with the script above. Both were benchmarked with
the identical `--local-only` argv.

Sizes: `target/local/rustfetch` = 2,868,480 bytes (2.8 MiB);
`target/release/rustfetch` = 2,943,096 bytes (2.9 MiB). The local binary is
~2.5% smaller.

Final script run from the repository root (`hyperfine --warmup 5 --runs 30`):

```
Benchmark 1: local (30 runs)
                mean     ±       σ          min     …     max
  Wall Time     15.8 ms  ±     4.8 ms      10.5 ms  …    23.9 ms
  Memory        11.9 MiB ±     0.2 MiB     11.5 MiB …    12.2 MiB

Benchmark 2: release (30 runs)
                mean     ±       σ          min     …     max
  Wall Time     13.2 ms  ±     1.2 ms      11.1 ms  …    16.5 ms           -16.8%
  Memory        11.2 MiB ±     0.1 MiB     10.9 MiB …    11.4 MiB            -6.3%
```

Two other runs of the identical script on the same binaries gave, for
local/release mean wall time: **10.0 ms ± 0.6 vs 10.9 ms ± 0.9** (local ~8%
faster) and **12.4 ms ± 2.0 vs 11.8 ms ± 3.2** (release ~5% faster). A longer
100-run measurement gave **11.0 ms ± 1.4 vs 10.6 ms ± 0.4**.

Interpretation: the results swing both ways because the host is shared and
this program is dominated by process startup and `/proc` reads. The build-time
flags give a reliably smaller binary; wall-time differences are within
run-to-run noise here. Do not expect a large speedup, and re-benchmark on your
own machine and workload.

## Uninstall / cleanup

```bash
# Remove just the local binary and its profile artifacts
trash target/local/rustfetch          # or: rm target/local/rustfetch
cargo clean --profile local           # removes local-profile build artifacts

# The normal release build is unaffected; remove it separately if desired
cargo clean --profile release
```

There is no installer integration: the local build is never installed
system-wide or added to a package. Delete the checked-in profile/script/docs
if you no longer want the option.
