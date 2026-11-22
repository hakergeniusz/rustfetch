# Windows guide — install, verify and use rustfetch

`rustfetch` for Windows is a **prebuilt, native x86_64 binary**. Release
`v0.2.5` publishes `rustfetch-windows-x86_64.exe` together with a
`rustfetch-windows-x86_64.exe.sha256` sidecar. Both are produced by the
[`Platform builds`](../.github/workflows/platform.yml) workflow on a **native
Windows CI runner**, and the binary is smoke-tested there before it is attached
to the release.

No Rust toolchain, no fastfetch installation and no administrator rights are
needed. fastfetch is never invoked at runtime.

The x86_64 executable also runs on Windows 11/arm64 through x64 emulation. A
native `rustfetch-windows-aarch64.exe` is attached to recent releases if you
prefer one; the PowerShell installer always fetches the x86_64 asset.

---

## One-liner install

Run in **PowerShell** (Windows PowerShell 5.1 or PowerShell 7+):

```powershell
irm https://raw.githubusercontent.com/hakergeniusz/rustfetch/main/installers/install.ps1 | iex
```

`irm` downloads the installer and `iex` runs it. It downloads the latest
release asset, verifies its SHA-256 against the published sidecar, installs
`rustfetch.exe` to `%LOCALAPPDATA%\Programs\rustfetch\` and adds that directory
to your **user** PATH. Only run one-liners from sources you trust; to review the
code first, use the manual/download-first flow below.

To pin a version or change the destination, save the script first:

```powershell
iwr https://raw.githubusercontent.com/hakergeniusz/rustfetch/main/installers/install.ps1 -OutFile install.ps1
powershell -ExecutionPolicy Bypass -File .\install.ps1 -Version v0.2.5
powershell -ExecutionPolicy Bypass -File .\install.ps1 -Version 0.2.5 -InstallDir "$env:USERPROFILE\Tools"
```

Both `-Version v0.2.5` and `-Version 0.2.5` select the same tag. Without
`-Version`, the installer uses the latest release.

---

## Manual download and verification

1. Open <https://github.com/hakergeniusz/rustfetch/releases/latest> and download
   **both** `rustfetch-windows-x86_64.exe` and
   `rustfetch-windows-x86_64.exe.sha256`.
2. Compare the published hash with the file you downloaded:

   ```powershell
   cd $env:USERPROFILE\Downloads
   Get-Content .\rustfetch-windows-x86_64.exe.sha256   # "<hash>  rustfetch-windows-x86_64.exe"
   Get-FileHash .\rustfetch-windows-x86_64.exe -Algorithm SHA256
   ```

   The two 64-character hex strings must match (case does not matter). If they
   do not, delete the download and try again.

3. Place the executable where you want it. For the same location the installer
   uses:

   ```powershell
   New-Item -ItemType Directory -Force "$env:LOCALAPPDATA\Programs\rustfetch" | Out-Null
   Copy-Item .\rustfetch-windows-x86_64.exe "$env:LOCALAPPDATA\Programs\rustfetch\rustfetch.exe"
   ```

   Add that folder to your user PATH via **Settings → System → About → Advanced
   system settings → Environment Variables → User variables → Path**, then
   open a new terminal. The binary is fully standalone: it links only system
   DLLs (kernel32, advapi32, ntdll, ws2_32) and ships no extra files.

---

## What the installer changes (PATH behavior)

- Binary: `%LOCALAPPDATA%\Programs\rustfetch\rustfetch.exe` by default, or
  `<InstallDir>\rustfetch.exe` when `-InstallDir` is given. Environment
  variables such as `%LOCALAPPDATA%` are expanded in `-InstallDir`.
- PATH: the install directory is appended to the **user** PATH stored in
  `HKCU\Environment` — never to the system PATH. The check is case-insensitive
  and ignores trailing separators, and it expands `%VAR%`-style entries, so
  repeated installs do not create duplicates. The current PowerShell session is
  updated as well.
- No elevation is required: everything is written under your user profile and
  `HKCU`.
- SHA-256: if the sidecar is present but the hash does not match, the script
  aborts and installs nothing. If no sidecar is published, it installs anyway
  and prints the downloaded file's hash for transparency.
- Uninstall removes only `rustfetch.exe` and its user PATH entry; it leaves the
  directory (and any other files you put there) in place.

---

## Verify the installation

Open a **new** terminal so the updated PATH is inherited, then run:

```powershell
rustfetch --version                              # rustfetch 0.2.5 (x86_64)
rustfetch --hyfetch-preset transgender            # colored Windows logo
rustfetch --list-modules                         # valid module names
rustfetch os cpu memory                          # positional module selection
rustfetch --pipe                                 # plain key: value lines
rustfetch --format json                          # JSON output
```

If `rustfetch` is not recognized yet, call the binary directly to confirm the
install itself worked:

```powershell
& "$env:LOCALAPPDATA\Programs\rustfetch\rustfetch.exe" --version
```

Colors work in Windows Terminal and modern PowerShell/console hosts with
virtual-terminal processing enabled. They are intentionally disabled for
`--pipe`, `--format json`, when `NO_COLOR` is set, and when stdout is
redirected.

---

## Supported modules and flags

The Windows port supports **11 core modules**:

`title`, `os`, `kernel`, `uptime`, `shell`, `terminal`, `cpu`, `gpu`, `memory`,
`disk`, `localip`

`ip` is accepted as an alias for `localip`, so `--list-modules` prints 12 names
(11 modules plus that alias). Pass module names as positional arguments to print
only those rows; with no arguments the default set (all 11) is printed.

Flags:

| Flag | Purpose |
|---|---|
| `--pipe` | No ANSI colors or escapes (script-friendly `key: value` lines). |
| `--json`, `--format json` | One JSON object with the core modules. |
| `--list-modules` | Print the valid module names. |
| `--hyfetch-preset NAME` | Color the logo with a pride-flag palette. |
| `--logo NAME\|none` | Built-in logo: `windows`, `generic` or `none`. |
| `--config PATH` | Read a JSONC config (default `%APPDATA%\fastfetch\config.jsonc`). |
| `--help`, `-h` | Show help. |
| `--version`, `-V` | Print version and architecture. |

Both `--flag value` and `--flag=value` work. Available HyFetch presets:
`rainbow`, `transgender`, `progress`, `random`, `nonbinary`, `bisexual`,
`pansexual`, `agender`, `lesbian`, `asexual`, `aromantic`.

The config parser understands only a `modules` array and
`logo.hyfetch.preset`; command-line flags override config values. For example:

```jsonc
{
  "logo": { "hyfetch": { "preset": "transgender" } },
  "modules": ["title", "os", "kernel", "uptime", "cpu", "memory", "disk", "localip"]
}
```

---

## Limitations compared with Linux

The Linux build (the repository root crate) is the full fastfetch 2.71.0 parity
build: all 76 modules, the complete custom `format` engine, exact HyFetch
colors and the full JSON schema. The Windows binary is a reduced native port:

- **Only the 11 core modules above.** No GPU details beyond a registry driver
  description, and no display, WM/DE, packages, battery, Wi-Fi, media, fonts,
  themes or other fastfetch modules.
- **Basic HyFetch colors.** Preset colors are used verbatim as RGB; the OKHSL
  re-lightening and byte-for-byte HyFetch 2.1.1 gradient are Linux-only.
- **Reduced JSON schema** — `--format json` emits only the core modules, not
  fastfetch's full module/JSON surface.
- **No custom `format` engine** (conditionals, padding, ranges, custom keys).
- **No zsh completion on Windows** (shell completion is a Linux zsh feature and
  is irrelevant here), no watch mode, no `--gen-config`, no image-protocol
  logos, and none of the other ~109 fastfetch flags.
- **Limited config support** (modules list and HyFetch preset only).
- **No byte-for-byte fastfetch parity** on Windows; output is not compared
  against fastfetch 2.71.0. If you need full parity, use the Linux build
  (native, WSL or a VM).

See [`platform-support.md`](platform-support.md) for the broader support
matrix; `rustfetch --help` and `rustfetch --list-modules` on the target machine
are authoritative for the Windows CLI surface.

---

## Uninstall

If you installed with the script:

```powershell
powershell -ExecutionPolicy Bypass -File .\install.ps1 -Uninstall
# custom directory:
powershell -ExecutionPolicy Bypass -File .\install.ps1 -InstallDir "$env:USERPROFILE\Tools" -Uninstall
```

This removes `rustfetch.exe` and its user PATH entry but keeps the directory.
For a manual install, delete the `.exe` and remove its directory from the user
PATH using the Environment Variables dialog linked above.

---

## Troubleshooting

- **"running scripts is disabled on this system" (execution policy).** Use the
  download-first form with an explicit bypass for that one file:

  ```powershell
  iwr https://raw.githubusercontent.com/hakergeniusz/rustfetch/main/installers/install.ps1 -OutFile install.ps1
  powershell -ExecutionPolicy Bypass -File .\install.ps1
  ```

  Alternatively allow locally created/remote-signed scripts for your user only:
  `Set-ExecutionPolicy -Scope CurrentUser RemoteSigned`.

- **Defender / SmartScreen warning.** The binary is not code-signed, so Windows
  may show "Windows protected your PC" or quarantine it. Verify the SHA-256
  against the published sidecar first (see above); if it matches, choose
  **More info → Run anyway**. If Defender removed the file, check
  **Windows Security → Protection history**, restore it, and add
  `%LOCALAPPDATA%\Programs\rustfetch` to your exclusions before reinstalling.

- **`rustfetch` is not recognized / PATH not refreshed.** Open a new terminal;
  the script only changes the registry for newly started processes and does not
  broadcast a setting-change message to already-running apps. If it still
  fails, sign out and back in (or restart Explorer), then check
  `echo $env:Path` and the `Path` value under `HKCU\Environment`. Re-running the
  installer re-adds a missing entry without duplicating an existing one.

- **Hash mismatch / install aborted.** Nothing was installed. Re-download the
  asset and sidecar; if they keep disagreeing, check for a proxy or corporate
  CDN rewriting the download and verify against the release page directly.

- **"Could not write ... close any running rustfetch instance".** Windows locks
  a running `.exe`; close rustfetch (or its terminal) and run the installer
  again.

- **No colors.** Use Windows Terminal or a modern console host; colors are
  deliberately disabled by `--pipe`, `--format json`, `NO_COLOR`, or redirected
  output.

- **Pinned release not found.** The installer turns `0.2.5` into the tag
  `v0.2.5`; confirm that exact tag exists on the releases page.
