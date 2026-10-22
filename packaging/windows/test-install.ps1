param(
    [Parameter(Mandatory = $true)][ValidateSet('x64', 'arm64')][string] $Arch,
    [Parameter(Mandatory = $true)][ValidatePattern('^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$')][string] $Version
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Assert-ContainedPath([string] $Base, [string] $Path) {
    if (-not [IO.Path]::IsPathFullyQualified($Base) -or -not [IO.Path]::IsPathFullyQualified($Path)) {
        throw 'Roundtrip paths must be absolute.'
    }
    $Boundary = [IO.Path]::GetFullPath($Base).TrimEnd([char[]]@('\', '/'))
    $Candidate = [IO.Path]::GetFullPath($Path)
    if ($Boundary -eq [IO.Path]::GetPathRoot($Boundary).TrimEnd([char[]]@('\', '/')) -or
        -not $Candidate.StartsWith("$Boundary$([IO.Path]::DirectorySeparatorChar)", [StringComparison]::OrdinalIgnoreCase)) {
        throw 'A roundtrip path escaped its dedicated temporary directory.'
    }
    return $Candidate
}

function Assert-RegularDirectory([string] $Path) {
    if (-not [IO.Path]::IsPathFullyQualified($Path)) { throw 'Expected an absolute directory.' }
    $Current = [IO.Path]::GetFullPath($Path)
    while ($Current) {
        $Item = Get-Item -LiteralPath $Current -Force
        if (-not $Item.PSIsContainer -or ($Item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            throw "A roundtrip directory is redirected or not a directory: $Current"
        }
        $Current = [IO.Path]::GetDirectoryName($Current)
    }
}

function Assert-RegularFile([string] $Path) {
    Assert-RegularDirectory ([IO.Path]::GetDirectoryName($Path))
    $Item = Get-Item -LiteralPath $Path -Force
    if ($Item.PSIsContainer -or ($Item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw "A roundtrip file is redirected or not a regular file: $Path"
    }
}

function Get-InstallerRecords {
    foreach ($View in [Microsoft.Win32.RegistryView]::Registry64, [Microsoft.Win32.RegistryView]::Registry32) {
        $User = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::CurrentUser, $View)
        try {
            foreach ($Name in 'Software\Craftworks', 'Software\Microsoft\Windows\CurrentVersion\Uninstall\Craftworks') {
                $Key = $User.OpenSubKey($Name, $false)
                if ($null -ne $Key) {
                    try {
                        $Values = @{}
                        foreach ($ValueName in $Key.GetValueNames()) { $Values[$ValueName] = $Key.GetValue($ValueName) }
                        [pscustomobject]@{ View = $View; Name = $Name; Values = $Values }
                    } finally { $Key.Dispose() }
                }
            }
        } finally { $User.Dispose() }
    }
}

function Invoke-CheckedProcess([string] $File, [string[]] $Arguments) {
    $Process = Start-Process -FilePath $File -ArgumentList $Arguments -PassThru -WindowStyle Hidden
    if (-not $Process.WaitForExit(120000)) { throw "Timed out waiting for the roundtrip process: $File" }
    $Process.Refresh()
    if ($Process.ExitCode -ne 0) { throw "The roundtrip process failed ($($Process.ExitCode)): $File" }
}

# This check is deliberately unavailable on a developer's Windows account.
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted' -or $env:RUNNER_OS -ne 'Windows') {
    throw 'The installer roundtrip may run only on a fresh GitHub-hosted Windows runner.'
}
if (@(Get-InstallerRecords).Count -ne 0) { throw 'Craftworks installer records already exist; no installer was run.' }

$Programs = [Environment]::GetFolderPath([Environment+SpecialFolder]::Programs)
Assert-RegularDirectory $Programs
$Shortcut = Join-Path $Programs 'Craftworks.lnk'
if (Test-Path -LiteralPath $Shortcut) { throw 'A Craftworks Start menu shortcut already exists; no installer was run.' }

$Root = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../..')).Path
$Setup = Join-Path $Root "dist/release/craftworks-$Version-windows-$Arch-setup.exe"
Assert-RegularFile $Setup
if (-not [IO.Path]::IsPathFullyQualified($env:RUNNER_TEMP)) { throw 'RUNNER_TEMP must be absolute.' }
$TaskRunnerTemp = [IO.Path]::GetFullPath($env:RUNNER_TEMP)
Assert-RegularDirectory $TaskRunnerTemp
$TestRoot = Assert-ContainedPath $TaskRunnerTemp (Join-Path $TaskRunnerTemp "craftworks nsis roundtrip $([guid]::NewGuid())")
if (Test-Path -LiteralPath $TestRoot) { throw 'The roundtrip directory already exists.' }
New-Item -ItemType Directory -Path $TestRoot | Out-Null
Assert-RegularDirectory $TestRoot
$InstallRoot = Assert-ContainedPath $TestRoot (Join-Path $TestRoot 'installed launcher')
$Inside = Assert-ContainedPath $InstallRoot (Join-Path $InstallRoot 'unrelated-user-file.txt')
$Outside = Assert-ContainedPath $TestRoot (Join-Path $TestRoot 'unrelated-outside-file.txt')
$Stdout = Assert-ContainedPath $TestRoot (Join-Path $TestRoot 'version.txt')
$Stderr = Assert-ContainedPath $TestRoot (Join-Path $TestRoot 'version-error.txt')
$Sentinel = "Keep unrelated files $([guid]::NewGuid())"
[IO.File]::WriteAllText($Outside, $Sentinel)

# NSIS consumes everything after /D= as the destination, so this argument stays last.
Invoke-CheckedProcess $Setup @('/S', "/D=$InstallRoot")
Assert-RegularDirectory $InstallRoot
$Expected = @('LICENSE-APACHE', 'LICENSE-MIT', 'README.md', 'Uninstall.exe', 'craftworks.exe') | Sort-Object
$Actual = @(Get-ChildItem -LiteralPath $InstallRoot -Force | Select-Object -ExpandProperty Name | Sort-Object)
if (@(Compare-Object $Expected $Actual).Count -ne 0) { throw 'The installed file inventory differs from the NSIS payload.' }
foreach ($Name in $Expected) { Assert-RegularFile (Join-Path $InstallRoot $Name) }
foreach ($Name in 'LICENSE-APACHE', 'LICENSE-MIT', 'README.md') {
    if ((Get-FileHash -LiteralPath (Join-Path $InstallRoot $Name)).Hash -ne (Get-FileHash -LiteralPath (Join-Path $Root $Name)).Hash) {
        throw "The installed $Name differs from this source package."
    }
}
$Binary = Join-Path $InstallRoot 'craftworks.exe'
$Target = if ($Arch -eq 'arm64') { 'aarch64-pc-windows-msvc' } else { 'x86_64-pc-windows-msvc' }
$TargetBase = if ($env:CARGO_TARGET_DIR) { [IO.Path]::GetFullPath($env:CARGO_TARGET_DIR, $Root) } else { Join-Path $Root 'target' }
$BuiltBinary = Join-Path $TargetBase "$Target/release/craftworks.exe"
Assert-RegularFile $BuiltBinary
if ((Get-FileHash -LiteralPath $Binary).Hash -ne (Get-FileHash -LiteralPath $BuiltBinary).Hash) { throw 'The installer did not install the current binary.' }
$Process = Start-Process -FilePath $Binary -ArgumentList '--version' -PassThru -WindowStyle Hidden -RedirectStandardOutput $Stdout -RedirectStandardError $Stderr
if (-not $Process.WaitForExit(30000)) { throw 'The installed binary version check timed out.' }
$Process.Refresh()
if ($Process.ExitCode -ne 0 -or (Get-Content -LiteralPath $Stdout -Raw).Trim() -ne "Craftworks $Version") {
    throw 'The installed binary reported an unexpected version or failed.'
}

$Records = @(Get-InstallerRecords)
if (@($Records | Where-Object Name -eq 'Software\Craftworks').Count -eq 0 -or
    @($Records | Where-Object Name -eq 'Software\Microsoft\Windows\CurrentVersion\Uninstall\Craftworks').Count -eq 0) {
    throw 'The installer did not register both per-user records.'
}
foreach ($Record in $Records) {
    if ($Record.Name -eq 'Software\Craftworks') {
        if ($Record.Values['InstallDir'] -ne $InstallRoot) { throw 'Unexpected registered InstallDir.' }
    } elseif ($Record.Values['InstallLocation'] -ne $InstallRoot -or $Record.Values['DisplayName'] -ne 'Craftworks' -or
        $Record.Values['DisplayVersion'] -ne $Version -or $Record.Values['Publisher'] -ne 'Craftworks contributors' -or
        $Record.Values['NoModify'] -ne 1 -or $Record.Values['NoRepair'] -ne 1 -or
        $Record.Values['UninstallString'] -ne "`"$InstallRoot\Uninstall.exe`"") {
        throw 'Unexpected registered installer metadata.'
    }
}
Assert-RegularFile $Shortcut
$Shell = New-Object -ComObject WScript.Shell
$Link = $Shell.CreateShortcut($Shortcut)
if ($Link.TargetPath -ne $Binary) { throw 'The Start menu shortcut points to a different binary.' }
[Runtime.InteropServices.Marshal]::FinalReleaseComObject($Link) | Out-Null
[Runtime.InteropServices.Marshal]::FinalReleaseComObject($Shell) | Out-Null

[IO.File]::WriteAllText($Inside, $Sentinel)
$Uninstaller = Assert-ContainedPath $InstallRoot (Join-Path $InstallRoot 'Uninstall.exe')
Assert-RegularFile $Uninstaller
# Never execute the registry's command string. NSIS may copy itself to a temporary process.
Invoke-CheckedProcess $Uninstaller @('/S')
$Deadline = [DateTime]::UtcNow.AddSeconds(60)
do {
    $Remaining = @($Expected | Where-Object { Test-Path -LiteralPath (Join-Path $InstallRoot $_) })
    $Removed = $Remaining.Count -eq 0 -and -not (Test-Path -LiteralPath $Shortcut) -and @(Get-InstallerRecords).Count -eq 0
    if (-not $Removed) { Start-Sleep -Milliseconds 200 }
} while (-not $Removed -and [DateTime]::UtcNow -lt $Deadline)
if (-not $Removed) { throw 'The installed uninstaller did not remove its payload, shortcut and registry records.' }
Assert-RegularDirectory $InstallRoot
foreach ($Path in $Inside, $Outside) {
    Assert-RegularFile $Path
    if ([IO.File]::ReadAllText($Path) -ne $Sentinel) { throw 'Uninstall changed an unrelated sentinel.' }
}
$Remaining = @(Get-ChildItem -LiteralPath $InstallRoot -Force | Select-Object -ExpandProperty Name)
if ($Remaining.Count -ne 1 -or $Remaining[0] -ne 'unrelated-user-file.txt') { throw 'Uninstall left unexpected installation files.' }

# Cleanup is intentionally explicit and nonrecursive; unexpected files remain for diagnosis.
foreach ($Path in $Inside, $Outside, $Stdout, $Stderr) {
    $null = Assert-ContainedPath $TestRoot $Path
    Assert-RegularFile $Path
    Remove-Item -LiteralPath $Path -Force
}
foreach ($Path in $InstallRoot, $TestRoot) {
    $null = Assert-ContainedPath $TaskRunnerTemp $Path
    Assert-RegularDirectory $Path
    if (@(Get-ChildItem -LiteralPath $Path -Force).Count -ne 0) { throw 'The roundtrip directory is not empty; it was kept.' }
    [IO.Directory]::Delete($Path, $false)
}
Write-Output "Windows $Arch installer roundtrip passed for Craftworks $Version; unrelated files were preserved."
