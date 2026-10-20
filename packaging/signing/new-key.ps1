param(
    [Parameter(Mandatory = $true)]
    [string]$OutputDirectory,
    [string]$OpenSsl
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
    throw 'Use new-key.sh on Linux. This script protects the key with Windows file permissions.'
}
if ($OutputDirectory -notmatch '^(?:[A-Za-z]:[\\/]|\\\\)') {
    throw 'OutputDirectory must be an absolute path outside the repository.'
}
$destination = [IO.Path]::GetFullPath($OutputDirectory).TrimEnd('\', '/')
$repository = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..')).TrimEnd('\', '/')
if ($destination.Equals($repository, [StringComparison]::OrdinalIgnoreCase) -or
    $destination.StartsWith($repository + '\', [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Store signing keys outside the repository.'
}
if (Test-Path -LiteralPath $destination) {
    throw 'The output directory already exists. Refusing to replace any existing key or file.'
}
$parent = [IO.Path]::GetDirectoryName($destination)
if (-not (Test-Path -LiteralPath $parent -PathType Container)) {
    throw 'Create the parent directory first, then choose a new output directory within it.'
}
# Reject junctions/symlinks so an external-looking path cannot put the key in this checkout.
$ancestor = Get-Item -LiteralPath $parent -Force
while ($null -ne $ancestor) {
    if (($ancestor.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw 'Choose an output path with no junctions or symbolic links in its parent directories.'
    }
    $ancestor = $ancestor.Parent
}

if (-not $OpenSsl) {
    $command = Get-Command openssl -ErrorAction SilentlyContinue
    if ($command) {
        $OpenSsl = $command.Source
    } else {
        $gitOpenSsl = Join-Path $env:ProgramFiles 'Git/usr/bin/openssl.exe'
        if (Test-Path -LiteralPath $gitOpenSsl -PathType Leaf) { $OpenSsl = $gitOpenSsl }
    }
}
if (-not $OpenSsl) { throw 'OpenSSL is required. Install Git for Windows or pass -OpenSsl with its full path.' }
& $OpenSsl version
if ($LASTEXITCODE -ne 0) { throw 'Could not run OpenSSL.' }

# Secure the empty folder before writing private material. No administrator access is needed.
$null = New-Item -ItemType Directory -Path $destination
$userSid = [Security.Principal.WindowsIdentity]::GetCurrent().User
$systemSid = [Security.Principal.SecurityIdentifier]::new('S-1-5-18')
$permissions = [Security.AccessControl.DirectorySecurity]::new()
$permissions.SetAccessRuleProtection($true, $false)
$permissions.SetOwner($userSid)
foreach ($sid in @($userSid, $systemSid)) {
    $rule = [Security.AccessControl.FileSystemAccessRule]::new(
        $sid, 'FullControl', 'ContainerInherit, ObjectInherit', 'None', 'Allow')
    $null = $permissions.AddAccessRule($rule)
}
Set-Acl -LiteralPath $destination -AclObject $permissions

$privateKey = Join-Path $destination 'craft-update-key.pem'
$publicDer = Join-Path $destination 'craft-update-public.der'
$publicText = Join-Path $destination 'craft-update-public-key.txt'
& $OpenSsl genpkey -algorithm ed25519 -out $privateKey
if ($LASTEXITCODE -ne 0) { throw 'OpenSSL could not generate the key. Inspect the protected output directory before retrying.' }
& $OpenSsl pkey -in $privateKey -pubout -outform DER -out $publicDer
if ($LASTEXITCODE -ne 0) { throw 'OpenSSL could not export the public key. The private key remains in the protected output directory.' }
$publicKey = [Convert]::ToBase64String([IO.File]::ReadAllBytes($publicDer))
[IO.File]::WriteAllText($publicText, $publicKey + [Environment]::NewLine, [Text.Encoding]::ASCII)

Write-Output "Private key saved in $privateKey. Keep it private and back it up securely."
Write-Output 'Public key (safe to share):'
Write-Output $publicKey
Write-Output 'In each treeforjs repository: pdfcraft, vectorcraft, photocraft, craftworks:'
Write-Output '  Settings > Environments > release > UPDATE_SIGNING_KEY secret = the whole PEM file.'
Write-Output '  Settings > Secrets and variables > Actions > Variables > CRAFT_UPDATE_PUBLIC_KEY = the public key above.'
Write-Output 'See docs/updates.md for release branches, publishing and rebuilding Craftworks.'
