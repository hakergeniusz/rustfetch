# Installers for Windows and macOS

These installers fetch the **prebuilt** `rustfetch` binary from the GitHub
releases of [hakergeniusz/rustfetch](https://github.com/hakergeniusz/rustfetch).
**fastfetch is NOT required at runtime** — no fastfetch package, binary or
configuration is needed. (An existing `~/.config/fastfetch/config.jsonc` may be
read as a compatibility preset, but fastfetch itself is never invoked.)

Release assets used here:

| Platform | Asset name |
|---|---|
| Windows x86_64 | `rustfetch-windows-x86_64.exe` |
| macOS x86_64 | `rustfetch-macos-universal` (see assumption below) |

## Windows — PowerShell installer

The Windows asset `rustfetch-windows-x86_64.exe` and its `.sha256` sidecar are
built and **smoke-tested on native Windows CI** (the `windows` job of
`.github/workflows/platform.yml`) before being attached to a release. For a
full end-user walkthrough — install, verification, supported modules, limits,
uninstall and troubleshooting — see the [Windows guide](../docs/windows-guide.md).

`install.ps1` downloads via `Invoke-WebRequest`, verifies SHA-256 when a
`<asset>.sha256` sidecar is published, installs the binary and updates the
**user** PATH (never the system PATH). No administrator rights are required.

Prerequisites: Windows 10/11 with Windows PowerShell 5.1 or PowerShell 7+, and
internet access to GitHub. Nothing else needs to be installed.

One-liner from a terminal:

```powershell
irm https://raw.githubusercontent.com/hakergeniusz/rustfetch/main/installers/install.ps1 | iex
```

Or download the script first (also works when `iex` is blocked by policy):

```powershell
iwr https://raw.githubusercontent.com/hakergeniusz/rustfetch/main/installers/install.ps1 -OutFile install.ps1
powershell -ExecutionPolicy Bypass -File .\install.ps1
```

Common options:

```powershell
# Pin a release tag ("0.2.5" is accepted as "v0.2.5")
powershell -ExecutionPolicy Bypass -File .\install.ps1 -Version v0.2.5

# Custom install directory (environment variables are expanded)
powershell -ExecutionPolicy Bypass -File .\install.ps1 -InstallDir "$env:USERPROFILE\Tools"

# Uninstall: removes the binary and the user PATH entry
powershell -ExecutionPolicy Bypass -File .\install.ps1 -Uninstall
# ...or, for a custom directory:
powershell -ExecutionPolicy Bypass -File .\install.ps1 -InstallDir "$env:USERPROFILE\Tools" -Uninstall
```

Where things land:

- Binary: `%LOCALAPPDATA%\Programs\rustfetch\rustfetch.exe` by default, or
  `<InstallDir>\rustfetch.exe` with `-InstallDir`.
- PATH: that directory is appended to the **user** PATH (`HKCU\Environment`)
  if it is not already there. The current terminal is updated as well; new
  terminals inherit the change automatically.
- If a `.sha256` sidecar exists but the hash does not match, the script aborts
  and installs nothing. If no sidecar is published it installs anyway and
  prints the downloaded file's SHA-256 for transparency.
- Uninstall leaves the (now empty) install directory in place so unrelated
  files you put there are never deleted.
- The zsh completion shipped for Linux does not apply to Windows and is not
  installed here.

## macOS — Homebrew formula

`rustfetch.rb` installs the prebuilt release archive. Prerequisites: macOS with
Homebrew installed and internet access to GitHub. **No Rust toolchain needed**
because no source build is performed.

From a checkout of this repository:

```sh
brew install ./installers/rustfetch.rb
```

Or place the file in a tap at
`Formula/rustfetch.rb` (e.g. `$(brew --repository)/Library/Taps/hakergeniusz/homebrew-rustfetch/Formula/rustfetch.rb`)
and run:

```sh
brew tap hakergeniusz/rustfetch
brew trust hakergeniusz/rustfetch   # required by newer Homebrew for third-party taps
brew install rustfetch
brew upgrade rustfetch   # after updating the formula for a new release
```

Where things land:

- Binary: `$(brew --prefix)/bin/rustfetch` (symlinked from the Cellar).
- Zsh completion: `share/zsh/site-functions/_rustfetch` when the archive ships
  `completions/_rustfetch`. Add `$(brew --prefix)/share/zsh/site-functions` to
  your `$fpath` before `compinit` if completions do not load.
- Update `version`, the asset URL and `sha256` together for every new release;
  Homebrew refuses to install when the checksum does not match.

Uninstall with `brew uninstall rustfetch`.

### Assumptions to confirm against CI

The macOS asset name is assumed to be
`rustfetch-macos-universal`. If CI uploads a `.zip` instead, change the
extension in the `url` and the checksum accordingly — Homebrew unpacks both
formats. If CI only builds Apple Silicon, point the formula at that asset (and
its checksum). The Windows asset name `rustfetch-windows-x86_64.exe` is
confirmed by the native Windows CI job that stages and smoke-tests it. The
formula assumes the macOS archive contains `rustfetch` at its root plus
`completions/_rustfetch`.
