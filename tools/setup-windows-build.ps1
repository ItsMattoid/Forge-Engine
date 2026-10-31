<#
.SYNOPSIS
  Sets up what building Forge on Windows needs for physics (Box3D): CMake and libclang.

.DESCRIPTION
  Box3D is compiled from source by the `box3d-sys` crate, which needs
    * CMake on PATH, and
    * libclang (to generate the Rust bindings), found through the LIBCLANG_PATH environment variable.
  Visual Studio Build Tools (the C++ workload) are needed as for any Rust build on Windows.

  This script
    1. finds CMake (installing it with winget if it is missing) and puts it on the user PATH;
    2. finds libclang.dll (an LLVM install, or the libclang bundled in the `libclang` PyPI wheel, which it downloads
       into %LOCALAPPDATA%\forge-build-tools\libclang - no Python needed, a wheel is a zip) and sets the user variable LIBCLANG_PATH.
  Nothing else on the machine is changed. Open a NEW terminal (or restart VS Code) afterwards so it sees the settings.

  Run:  powershell -ExecutionPolicy Bypass -File tools\setup-windows-build.ps1
#>
$ErrorActionPreference = "Stop"

function Add-UserPath([string]$dir) {
    $user = [Environment]::GetEnvironmentVariable("Path", "User")
    if (($user -split ";") -notcontains $dir) {
        [Environment]::SetEnvironmentVariable("Path", ($user.TrimEnd(";") + ";" + $dir), "User")
        Write-Host "Added $dir to the user PATH."
    }
    if (($env:Path -split ";") -notcontains $dir) { $env:Path += ";$dir" }
}

# ---- CMake ----------------------------------------------------------------------------------------
$cmake = Get-Command cmake -ErrorAction SilentlyContinue
if (-not $cmake) {
    $known = "C:\Program Files\CMake\bin\cmake.exe"
    if (-not (Test-Path $known)) {
        Write-Host "CMake not found: installing it with winget."
        winget install --id Kitware.CMake -e --silent --accept-package-agreements --accept-source-agreements
    }
    if (-not (Test-Path $known)) { throw "CMake is not installed. Install it from https://cmake.org/download/ and run this script again." }
    Add-UserPath (Split-Path $known)
}
Write-Host "CMake: $((Get-Command cmake).Source)"

# ---- libclang -------------------------------------------------------------------------------------
function Test-Libclang([string]$dir) { $dir -and ((Test-Path "$dir\libclang.dll") -or (Test-Path "$dir\clang.dll")) }

$libclang = $env:LIBCLANG_PATH
if (-not (Test-Libclang $libclang)) {
    $libclang = $null
    foreach ($candidate in @("C:\Program Files\LLVM\bin", "C:\Program Files (x86)\LLVM\bin")) {
        if (Test-Libclang $candidate) { $libclang = $candidate; break }
    }
}
if (-not $libclang) {
    $target = Join-Path $env:LOCALAPPDATA "forge-build-tools\libclang"
    if (-not (Test-Libclang $target)) {
        Write-Host "No LLVM found: fetching libclang from the PyPI wheel."
        $work = Join-Path ([IO.Path]::GetTempPath()) ("forge-libclang-" + [guid]::NewGuid())
        New-Item -ItemType Directory -Force $work | Out-Null
        $meta = Invoke-RestMethod "https://pypi.org/pypi/libclang/json"
        $file = $meta.urls | Where-Object { $_.filename -like "*win_amd64.whl" } | Select-Object -First 1
        if (-not $file) { throw "no Windows libclang wheel found on PyPI; install LLVM instead (winget install LLVM.LLVM)" }
        $zip = Join-Path $work "libclang.zip"
        Invoke-WebRequest $file.url -OutFile $zip
        Expand-Archive $zip (Join-Path $work "x") -Force
        $dll = Get-ChildItem (Join-Path $work "x") -Recurse -Filter libclang.dll | Select-Object -First 1
        if (-not $dll) { throw "libclang.dll was not in the downloaded wheel" }
        New-Item -ItemType Directory -Force $target | Out-Null
        Copy-Item $dll.FullName $target -Force
        Remove-Item $work -Recurse -Force
    }
    $libclang = $target
}
[Environment]::SetEnvironmentVariable("LIBCLANG_PATH", $libclang, "User")
$env:LIBCLANG_PATH = $libclang
Write-Host "LIBCLANG_PATH: $libclang"

# ---- cargo -----------------------------------------------------------------------------------------
# Cargo reads ~/.cargo/config.toml on every run, so recording the locations there works from any terminal, even one
# that was opened before this script ran. (Variables already set in the environment win.)
$cmakeExe = (Get-Command cmake).Source -replace "\\", "/"
$libclangDir = $libclang -replace "\\", "/"
$cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE ".cargo" }
New-Item -ItemType Directory -Force $cargoHome | Out-Null
$config = Join-Path $cargoHome "config.toml"
$block = @"
# --- Forge: native build tools for physics (Box3D), written by tools/setup-windows-build.ps1 ---
[env]
CMAKE = { value = "$cmakeExe", force = false }
LIBCLANG_PATH = { value = "$libclangDir", force = false }
# --- end Forge ---
"@
$existing = if (Test-Path $config) { Get-Content $config -Raw } else { "" }
if ($existing -match "(?s)# --- Forge: .*?# --- end Forge ---") {
    $existing = [regex]::Replace($existing, "(?s)# --- Forge: .*?# --- end Forge ---", $block)
} elseif ($existing -match "(?m)^\[env\]") {
    throw "$config already has an [env] section: add CMAKE and LIBCLANG_PATH to it yourself:`n$block"
} else {
    $existing = ($existing.TrimEnd() + "`n`n" + $block).TrimStart()
}
[IO.File]::WriteAllText($config, $existing + "`n", (New-Object System.Text.UTF8Encoding($false)))
Write-Host "Cargo config: $config"
Write-Host ""
Write-Host "Done. Run: cargo build   (a terminal opened before this script ran works too)"
