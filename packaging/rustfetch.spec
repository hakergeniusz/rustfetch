Name:           rustfetch
Version:        0.2.4
Release:        1%{?dist}
Summary:        A fastfetch- and hyfetch-inspired system information tool written in Rust

License:        MIT
URL:            https://github.com/hakergeniusz/rustfetch
Source0:        %{name}-%{version}.tar.gz

BuildRequires:  rust
BuildRequires:  cargo
# The release binary links only libc, libm and libgcc_s at runtime.
Requires:       glibc
Requires:       libgcc
# fastfetch is NOT a runtime dependency.

%description
A fast, dependency-light system information tool written in Rust, inspired by
fastfetch and hyfetch. It mirrors fastfetch's logo database, colors and
modules, reading system information directly from procfs, sysfs, DMI and
pci.ids. The binary is self-contained apart from the standard C runtime
libraries.

%prep
%autosetup -n %{name}-%{version}

%build
export CARGO_TARGET_DIR="$PWD/target"
cargo build --release

%install
mkdir -p %{buildroot}%{_bindir}
install -m 0755 target/release/rustfetch %{buildroot}%{_bindir}/rustfetch
install -d %{buildroot}%{_licensedir}/%{name}
install -m 0644 LICENSE %{buildroot}%{_licensedir}/%{name}/LICENSE
install -d %{buildroot}%{_docdir}/%{name}
install -m 0644 README.md %{buildroot}%{_docdir}/%{name}/README.md

%files
%{_bindir}/rustfetch
%license %{_licensedir}/%{name}/LICENSE
%doc %{_docdir}/%{name}/README.md

%changelog
* Sat Nov 21 2026 hakergeniusz <hakergeniusz@users.noreply.github.com> - 0.1.0-1
- Initial release.
