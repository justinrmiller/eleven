# Build the Windows release package into dist/:
#   eleven-<version>-windows-<arch>.zip   (eleven.exe and README.md in one folder)
# Needs PowerShell 7 and the MSVC build tools. Run `pwsh scripts/package-windows.ps1`, or
# `make dist-windows`.
#Requires -Version 7.3
$ErrorActionPreference = 'Stop'
# Stop when a native command (cargo) fails, too.
$PSNativeCommandUseErrorActionPreference = $true
Set-StrictMode -Version Latest

if (-not $IsWindows) {
    throw 'package-windows.ps1 must be run on Windows'
}
Set-Location (Join-Path $PSScriptRoot '..')

# Like scripts/version.sh: --locked fails if Cargo.lock is out of date with Cargo.toml.
$version = (cargo pkgid --locked -p eleven) -replace '.*[#@]', ''
cargo build -p eleven --profile dist --locked

$exe = 'target/dist/eleven.exe'
# The build script embeds the icon and version only if it found a resource compiler; without
# them Explorer shows a generic icon, so a release must not ship that way.
$productVersion = (Get-Item $exe).VersionInfo.ProductVersion
if ($productVersion -ne $version) {
    throw "eleven.exe has no version resource (got '$productVersion'). Is the Windows SDK installed?"
}

# The architecture the binary was built for: the toolchain's host, e.g. x86_64-pc-windows-msvc.
$hostTriple = (rustc -vV | Select-String '^host: ').Line -replace '^host: ', ''
$arch = $hostTriple.Split('-')[0]
$name = "eleven-$version-windows-$arch"

$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ([System.IO.Path]::GetRandomFileName())
try {
    $stage = Join-Path $tmp $name
    New-Item -ItemType Directory -Path $stage | Out-Null
    Copy-Item $exe (Join-Path $stage 'eleven.exe')
    Copy-Item README.md (Join-Path $stage 'README.md')
    New-Item -ItemType Directory -Force -Path dist | Out-Null
    $zip = "dist/$name.zip"
    # Passing the folder itself keeps it as the archive's top level, like the Linux tarball.
    Compress-Archive -Path $stage -DestinationPath $zip -Force
    Write-Output "Built $zip"
}
finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
