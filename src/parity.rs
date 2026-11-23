//! Parity mode: make rustfetch behave exactly like the pinned fastfetch
//! reference (2.71.0) instead of applying rustfetch's own ergonomic defaults.
//!
//! Enabled by `RUSTFETCH_PARITY_VERSION` (the same switch that already pins
//! the version string in `cli.rs`). Current overrides:
//!   * `disableLinewrap` is forced to fastfetch's default of `false`, so no
//!     `ESC[?7l` / `ESC[?7h` toggles are emitted in TTY mode.
//!   * the Display module uses fastfetch's sysfs-DRM detection path rather
//!     than the mutter D-Bus path.
//!
//! Normal (non-parity) invocations are unchanged.

/// Whether this invocation should match the fastfetch reference byte for byte.
pub(crate) fn enabled() -> bool {
    std::env::var_os("RUSTFETCH_PARITY_VERSION").is_some()
}
