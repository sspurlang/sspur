# Install a prebuilt sspur.exe from GitHub releases.
#   irm https://raw.githubusercontent.com/sspurlang/sspur/main/install.ps1 | iex
# Environment: SSPUR_VERSION (default: latest), SSPUR_INSTALL_DIR (default: %LOCALAPPDATA%\sspur\bin),
#   SSPUR_DOWNLOAD_BASE (a mirror holding the release zips and SHA256SUMS)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$repo = 'sspurlang/sspur'
function Say($m) { Write-Host "sspur-install: $m" }
function Die($m) { Write-Host "sspur-install: $m" -ForegroundColor Red; throw "sspur-install failed: $m" }

$dest = if ($env:SSPUR_INSTALL_DIR) { $env:SSPUR_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'sspur\bin' }
$arch = switch ($env:PROCESSOR_ARCHITECTURE) {
    'AMD64' { 'x86_64' }
    'ARM64' { 'aarch64' }
    default { Die "unsupported CPU: $env:PROCESSOR_ARCHITECTURE" }
}
$target = "$arch-pc-windows-msvc"

$version = $env:SSPUR_VERSION
if (-not $version) {
    $version = (Invoke-RestMethod "https://api.github.com/repos/$repo/releases/latest").tag_name
    if (-not $version) { Die 'could not find the latest release; set SSPUR_VERSION' }
}
if (-not $version.StartsWith('v')) { $version = "v$version" }

$name = "sspur-$version-$target"
$base = if ($env:SSPUR_DOWNLOAD_BASE) { $env:SSPUR_DOWNLOAD_BASE } else { "https://github.com/$repo/releases/download/$version" }
$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("sspur-install-" + [System.Guid]::NewGuid())
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
    Say "downloading $name.zip"
    $zip = Join-Path $tmp "$name.zip"
    $sums = Join-Path $tmp 'SHA256SUMS'
    try { Invoke-WebRequest -UseBasicParsing -Uri "$base/$name.zip" -OutFile $zip } catch { Die "download failed: $base/$name.zip" }
    try { Invoke-WebRequest -UseBasicParsing -Uri "$base/SHA256SUMS" -OutFile $sums } catch { Die "download failed: $base/SHA256SUMS" }

    $want = $null
    foreach ($line in Get-Content $sums) {
        $parts = $line -split '\s+', 2
        if ($parts.Count -eq 2 -and $parts[1].TrimStart('*') -eq "$name.zip") { $want = $parts[0].ToLower() }
    }
    if (-not $want) { Die "no checksum for $name.zip in SHA256SUMS" }
    $got = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLower()
    if ($want -ne $got) { Die "checksum mismatch for $name.zip (expected $want, got $got)" }
    Say 'checksum ok'

    Expand-Archive -Path $zip -DestinationPath $tmp -Force
    New-Item -ItemType Directory -Force -Path $dest | Out-Null
    Copy-Item -Force (Join-Path $tmp "$name\sspur.exe") (Join-Path $dest 'sspur.exe')
    $v = & (Join-Path $dest 'sspur.exe') --version
    Say "installed $v to $dest\sspur.exe"
} finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}

$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$parts = if ($userPath) { $userPath -split ';' } else { @() }
if ($parts -notcontains $dest) {
    $new = if ($userPath) { "$userPath;$dest" } else { $dest }
    [Environment]::SetEnvironmentVariable('Path', $new, 'User')
    $env:Path = "$env:Path;$dest"
    Say "added $dest to your user PATH (open a new terminal to pick it up)"
}
if (-not (Get-Command clang -ErrorAction SilentlyContinue) -and -not (Test-Path "$env:ProgramFiles\LLVM\bin\clang.exe")) {
    Say 'clang was not found; sspur needs LLVM (winget install LLVM.LLVM) and the Visual Studio Build Tools to compile native code'
}
