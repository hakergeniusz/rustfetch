# rustfetch - native FreeBSD build

Standalone, libc-only port of [rustfetch](../..) for FreeBSD. Its own binary
is also named `rustfetch`; the Linux root crate is untouched.

This is a reduced native build, **not** a fastfetch parity build. See
[`docs/freebsd.md`](../../docs/freebsd.md) for build instructions, the module
list and known limitations.

```sh
cd platform/freebsd
cargo build --release
./target/release/rustfetch
```

Cross-check from any host with the Rust std target installed (no linker
needed for `cargo check`):

```sh
rustup target add x86_64-unknown-freebsd
cargo --locked check --target x86_64-unknown-freebsd
```

License: MIT.
