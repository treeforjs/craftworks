param(
    [ValidateSet('x64', 'arm64')][string] $Arch = 'x64',
    [switch] $SkipBuild,
    [string] $BinaryPath,
    [string] $MakeNsis = 'makensis'
)

$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$Cargo = Get-Content -Raw (Join-Path $Root 'apps/craftworks/Cargo.toml')
$Match = [regex]::Match($Cargo, '(?m)^version\s*=\s*"([^"]+)"')
if (-not $Match.Success) { throw 'Could not read the Craftworks version.' }
$Version = $Match.Groups[1].Value
if ($Version -notmatch '^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$') { throw 'Invalid package version.' }
$Target = if ($Arch -eq 'arm64') { 'aarch64-pc-windows-msvc' } else { 'x86_64-pc-windows-msvc' }
$TargetBase = if ($env:CARGO_TARGET_DIR) { [IO.Path]::GetFullPath($env:CARGO_TARGET_DIR, $Root) } else { Join-Path $Root 'target' }

Push-Location $Root
try {
    if ($BinaryPath -and -not $SkipBuild) { throw '-BinaryPath requires -SkipBuild.' }
    if (-not $SkipBuild) {
        # One binary, including the C runtime, so the installer needs no redistributable.
        $OldRustFlags = $env:RUSTFLAGS
        try {
            $env:RUSTFLAGS = "$OldRustFlags -C target-feature=+crt-static".Trim()
            & cargo build --locked --release --package craftworks --target $Target
            if ($LASTEXITCODE -ne 0) { throw 'Craftworks build failed.' }
        } finally { $env:RUSTFLAGS = $OldRustFlags }
    }
    $Binary = if ($BinaryPath) { [IO.Path]::GetFullPath($BinaryPath, $Root) } else { Join-Path $TargetBase "$Target/release/craftworks.exe" }
    if (-not (Test-Path -LiteralPath $Binary -PathType Leaf)) { throw "Missing binary: $Binary" }
    # Check the PE machine field so a prebuilt x64 binary cannot ship labelled ARM64.
    $Reader = [IO.BinaryReader]::new([IO.File]::OpenRead($Binary))
    try {
        if ($Reader.ReadUInt16() -ne 0x5A4D) { throw 'The binary has no Windows executable header.' }
        $Reader.BaseStream.Position = 0x3C
        $PeOffset = $Reader.ReadUInt32()
        if ($PeOffset -gt ($Reader.BaseStream.Length - 6)) { throw 'The binary has an invalid PE header offset.' }
        $Reader.BaseStream.Position = $PeOffset
        if ($Reader.ReadUInt32() -ne 0x00004550) { throw 'The binary has no PE header.' }
        $Machine = $Reader.ReadUInt16()
        $ExpectedMachine = if ($Arch -eq 'arm64') { 0xAA64 } else { 0x8664 }
        if ($Machine -ne $ExpectedMachine) { throw "The binary architecture does not match -Arch $Arch." }
    } finally { $Reader.Dispose() }
    # A fresh, bounded staging directory avoids deleting a developer's build files.
    $Stage = Join-Path $TargetBase "craftworks-package/$([guid]::NewGuid())/craftworks-$Version-windows-$Arch-portable"
    New-Item -ItemType Directory -Path $Stage -Force | Out-Null
    Copy-Item -LiteralPath $Binary -Destination (Join-Path $Stage 'craftworks.exe')
    foreach ($Name in 'LICENSE-MIT', 'LICENSE-APACHE', 'README.md') {
        Copy-Item -LiteralPath (Join-Path $Root $Name) -Destination $Stage
    }
    $Dist = Join-Path $Root 'dist/release'
    New-Item -ItemType Directory -Path $Dist -Force | Out-Null
    $Zip = Join-Path $Dist "craftworks-$Version-windows-$Arch-portable.zip"
    Compress-Archive -LiteralPath $Stage -DestinationPath $Zip -Force
    $Setup = Join-Path $Dist "craftworks-$Version-windows-$Arch-setup.exe"
    & $MakeNsis "/DVERSION=$Version" "/DPACKAGE_DIR=$Stage" "/DOUT_FILE=$Setup" (Join-Path $PSScriptRoot 'craftworks.nsi')
    if ($LASTEXITCODE -ne 0) { throw 'NSIS packaging failed.' }
    Get-Item -LiteralPath $Setup, $Zip | Select-Object Name, Length
} finally { Pop-Location }
