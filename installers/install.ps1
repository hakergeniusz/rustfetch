<#
.SYNOPSIS
    Install or uninstall rustfetch for the current Windows user (no admin needed).

.DESCRIPTION
    Downloads the prebuilt rustfetch-windows-x86_64.exe asset from the latest
    GitHub release, or from a pinned release tag when -Version is given.
    Verifies SHA-256 when a <asset>.sha256 sidecar is published, installs the
    binary to %LOCALAPPDATA%\Programs\rustfetch\rustfetch.exe by default, and
    adds the install directory to the USER PATH (never the system PATH).

.PARAMETER InstallDir
    Directory to install rustfetch.exe into. Defaults to
    %LOCALAPPDATA%\Programs\rustfetch. Environment variables are expanded.

.PARAMETER Version
    Release tag to pin, e.g. v0.2.5 or 0.2.5 (a leading "v" is added if
    missing). Defaults to the latest release.

.PARAMETER Uninstall
    Remove rustfetch.exe from the install directory and remove that directory
    from the USER PATH.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File .\install.ps1

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File .\install.ps1 -Version v0.2.5 -InstallDir "$env:USERPROFILE\Tools"

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File .\install.ps1 -Uninstall
#>
[CmdletBinding()]
param(
    [string]$InstallDir,
    [string]$Version,
    [switch]$Uninstall
)

$ErrorActionPreference = 'Stop'

$RepoOwner = 'hakergeniusz'
$RepoName  = 'rustfetch'
$AssetName = 'rustfetch-windows-x86_64.exe'
$ExeName   = 'rustfetch.exe'

function Write-Step {
    param([string]$Message)
    Write-Host "[rustfetch] $Message"
}

function Write-Detail {
    param([string]$Message)
    Write-Host "[rustfetch]   $Message" -ForegroundColor Gray
}

function Write-Success {
    param([string]$Message)
    Write-Host "[rustfetch] $Message" -ForegroundColor Green
}

function Get-InstallDirectory {
    if (-not [string]::IsNullOrWhiteSpace($InstallDir)) {
        $expanded = [Environment]::ExpandEnvironmentVariables($InstallDir)
        return $expanded.TrimEnd('\', '/')
    }
    $local = $env:LOCALAPPDATA
    if ([string]::IsNullOrWhiteSpace($local)) {
        $local = Join-Path $env:USERPROFILE 'AppData\Local'
    }
    return Join-Path (Join-Path $local 'Programs') 'rustfetch'
}

function Get-ReleaseBaseUrl {
    if (-not [string]::IsNullOrWhiteSpace($Version)) {
        $tag = $Version.Trim()
        if ($tag -notmatch '^v') { $tag = "v$tag" }
        return "https://github.com/$RepoOwner/$RepoName/releases/download/$tag"
    }
    return "https://github.com/$RepoOwner/$RepoName/releases/latest/download"
}

function Test-DirectoryInEntries {
    param(
        [string[]]$Entries,
        [string]$Directory
    )
    foreach ($entry in $Entries) {
        if ([string]::IsNullOrWhiteSpace($entry)) { continue }
        $normalized = $entry.Trim().Trim('"').TrimEnd('\', '/')
        if ([string]::Equals($normalized, $Directory, [StringComparison]::OrdinalIgnoreCase)) {
            return $true
        }
    }
    return $false
}

function Add-UserPathEntry {
    param([string]$Directory)

    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if ([string]::IsNullOrEmpty($userPath)) {
        [Environment]::SetEnvironmentVariable('Path', $Directory, 'User')
        Write-Step 'Added the install directory to the user PATH.'
    } elseif (Test-DirectoryInEntries -Entries ($userPath -split ';') -Directory $Directory) {
        Write-Detail 'Install directory is already on the user PATH.'
    } else {
        $updated = $userPath.TrimEnd(';') + ';' + $Directory
        [Environment]::SetEnvironmentVariable('Path', $updated, 'User')
        Write-Step 'Added the install directory to the user PATH.'
    }

    # Make the binary usable in this terminal too (the registry change only
    # applies to newly started processes).
    if (-not (Test-DirectoryInEntries -Entries ($env:Path -split ';') -Directory $Directory)) {
        $env:Path = $env:Path.TrimEnd(';') + ';' + $Directory
    }
}

function Remove-UserPathEntry {
    param([string]$Directory)

    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if (-not [string]::IsNullOrEmpty($userPath)) {
        $kept = [System.Collections.Generic.List[string]]::new()
        $removed = $false
        foreach ($entry in ($userPath -split ';')) {
            if ([string]::IsNullOrWhiteSpace($entry)) { continue }
            if (Test-DirectoryInEntries -Entries @($entry) -Directory $Directory) {
                $removed = $true
            } else {
                $kept.Add($entry)
            }
        }
        if ($removed) {
            [Environment]::SetEnvironmentVariable('Path', ($kept -join ';'), 'User')
            Write-Step 'Removed the install directory from the user PATH.'
        } else {
            Write-Detail 'Install directory was not on the user PATH.'
        }
    } else {
        Write-Detail 'No user PATH entry to remove.'
    }

    $sessionKept = [System.Collections.Generic.List[string]]::new()
    foreach ($entry in ($env:Path -split ';')) {
        if ([string]::IsNullOrWhiteSpace($entry)) { continue }
        if (-not (Test-DirectoryInEntries -Entries @($entry) -Directory $Directory)) {
            $sessionKept.Add($entry)
        }
    }
    $env:Path = $sessionKept -join ';'
}

function Invoke-Download {
    param(
        [string]$Url,
        [string]$Destination,
        [switch]$Optional
    )

    $parameters = @{ Uri = $Url; OutFile = $Destination }
    if ($PSVersionTable.PSVersion.Major -lt 6) {
        # Required by Windows PowerShell 5.1 in non-IE environments.
        $parameters['UseBasicParsing'] = $true
    }
    try {
        Invoke-WebRequest @parameters | Out-Null
    } catch {
        if ($Optional) { return $false }
        throw "Failed to download $Url : $($_.Exception.Message)"
    }
    return $true
}

try {
    $targetDir = Get-InstallDirectory
    $targetExe = Join-Path $targetDir $ExeName
    $releaseBase = Get-ReleaseBaseUrl
    $assetUrl = "$releaseBase/$AssetName"

    if ($Uninstall) {
        Write-Step "Uninstalling rustfetch from $targetDir"
        if (Test-Path -LiteralPath $targetExe -PathType Leaf) {
            Remove-Item -LiteralPath $targetExe -Force
            Write-Step "Removed $targetExe"
        } else {
            Write-Detail "No binary found at $targetExe"
        }
        Remove-UserPathEntry -Directory $targetDir
        Write-Success "rustfetch uninstalled (the directory $targetDir was left in place)."
        return
    }

    $tempDir = Join-Path ([IO.Path]::GetTempPath()) ('rustfetch-install-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $tempDir -Force | Out-Null

    try {
        Write-Step "Downloading $assetUrl"
        $downloaded = Join-Path $tempDir $AssetName
        $null = Invoke-Download -Url $assetUrl -Destination $downloaded

        $size = (Get-Item -LiteralPath $downloaded).Length
        if ($size -lt 1024) {
            throw "Downloaded file is only $size bytes; refusing to install it. Check that the release asset $AssetName exists."
        }
        Write-Detail "Downloaded $size bytes."

        # SHA-256 sidecar is optional: <asset>.sha256 next to the asset.
        $sidecar = Join-Path $tempDir ($AssetName + '.sha256')
        if (Invoke-Download -Url "$assetUrl.sha256" -Destination $sidecar -Optional) {
            $sidecarContent = Get-Content -LiteralPath $sidecar -Raw
            $hashMatch = [regex]::Match($sidecarContent, '\b[0-9a-fA-F]{64}\b')
            if (-not $hashMatch.Success) {
                throw "The SHA-256 sidecar $assetUrl.sha256 does not contain a valid hash."
            }
            $expected = $hashMatch.Value.ToUpperInvariant()
            $actual = (Get-FileHash -LiteralPath $downloaded -Algorithm SHA256).Hash.ToUpperInvariant()
            if ($actual -ne $expected) {
                throw "SHA-256 mismatch for $AssetName. Expected $expected but got $actual. Nothing was installed."
            }
            Write-Step 'SHA-256 verified.'
        } else {
            Remove-Item -LiteralPath $sidecar -Force -ErrorAction SilentlyContinue
            $actual = (Get-FileHash -LiteralPath $downloaded -Algorithm SHA256).Hash.ToUpperInvariant()
            Write-Detail "No SHA-256 sidecar found; skipping verification. Downloaded SHA-256: $actual"
        }

        New-Item -ItemType Directory -Path $targetDir -Force | Out-Null
        try {
            Copy-Item -LiteralPath $downloaded -Destination $targetExe -Force
        } catch {
            throw "Could not write $targetExe (close any running rustfetch instance and check permissions): $($_.Exception.Message)"
        }
        Write-Step "Installed $targetExe"
    } finally {
        Remove-Item -LiteralPath $tempDir -Recurse -Force -ErrorAction SilentlyContinue
    }

    Add-UserPathEntry -Directory $targetDir
    Write-Success "rustfetch installed successfully: $targetExe"
    Write-Detail 'This terminal was updated too; new terminals inherit the user PATH automatically.'
} catch {
    Write-Host "[rustfetch] ERROR: $($_.Exception.Message)" -ForegroundColor Red
    throw
}
