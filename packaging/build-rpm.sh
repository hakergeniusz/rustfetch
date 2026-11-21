#!/usr/bin/env bash
# Build source + binary RPMs for rustfetch with rpmbuild.
# The spec builds from source (cargo build --release); this wrapper prepares
# a clean source tarball from git and sets a configurable topdir.
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"
VERSION="${VERSION:-0.1.0}"
RELEASE="${RELEASE:-1}"
TOPDIR="${RPM_TOPDIR:-${TOPDIR:-$ROOT/rpmbuild}}"
SPEC="$ROOT/packaging/rustfetch.spec"
NAME="rustfetch"
TARBALL="$NAME-$VERSION.tar.gz"

info() { printf '[*] %s\n' "$*"; }
ok()   { printf '[+] %s\n' "$*"; }
die()  { printf '[ERROR] %s\n' "$*" >&2; exit 1; }

command -v rpmbuild >/dev/null 2>&1 || die "rpmbuild not found (Fedora: 'sudo dnf install rpm-build'; openSUSE: 'sudo zypper install rpm-build')"
[[ -f "$SPEC" ]] || die "spec file not found: $SPEC"
git -C "$ROOT" rev-parse --git-dir >/dev/null 2>&1 || die "source tarball requires a git checkout (no git repo at $ROOT)"

info "RPM topdir: $TOPDIR"
mkdir -p "$TOPDIR"/{BUILD,RPMS,SOURCES,SPECS,SRPMS}

info "Creating source tarball $TARBALL from git HEAD"
git -C "$ROOT" archive --prefix="$NAME-$VERSION/" -o "$TOPDIR/SOURCES/$TARBALL" HEAD
install -m 0644 "$SPEC" "$TOPDIR/SPECS/rustfetch.spec"

info "Running rpmbuild -ba"
rpmbuild -ba --define "_topdir $TOPDIR" "$TOPDIR/SPECS/rustfetch.spec"
ok "RPMs are under $TOPDIR/RPMS and $TOPDIR/SRPMS"
