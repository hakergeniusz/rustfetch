# Maintainer: hakergeniusz
# Source-build PKGBUILD: build rustfetch from an upstream release tarball.
pkgname=rustfetch
pkgver=0.2.6
pkgrel=1
pkgdesc='A fastfetch- and hyfetch-inspired system information tool written in Rust'
arch=('x86_64' 'aarch64' 'armv7h' 'riscv64')
url='https://github.com/hakergeniusz/rustfetch'
license=('MIT')
# The release binary only needs libc, libm and libgcc_s at runtime.
# On Arch those are provided by glibc and gcc-libs. fastfetch is NOT required.
depends=('glibc' 'gcc-libs')
makedepends=('rust' 'cargo')
source=("$pkgname-$pkgver.tar.gz::https://github.com/hakergeniusz/rustfetch/archive/refs/tags/v$pkgver.tar.gz")
sha256sums=('SKIP')

_TARGET="$srcdir/target"

build() {
  cd "$pkgname-$pkgver"
  export CARGO_TARGET_DIR="$_TARGET"
  cargo build --release
}

check() {
  cd "$pkgname-$pkgver"
  export CARGO_TARGET_DIR="$_TARGET"
  "$_TARGET/release/rustfetch" --version
}

package() {
  cd "$pkgname-$pkgver"
  install -Dm755 "$_TARGET/release/rustfetch" "$pkgdir/usr/bin/rustfetch"
  install -Dm644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
  install -Dm644 README.md "$pkgdir/usr/share/doc/$pkgname/README.md"
  install -Dm644 completions/_rustfetch "$pkgdir/usr/share/zsh/site-functions/_rustfetch"
}
