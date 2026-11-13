[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$BinaryPath,
    [Parameter(Mandatory)][string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$version = (Get-Content -Raw -LiteralPath (Join-Path $root 'src-tauri/tauri.conf.json') | ConvertFrom-Json).version
if ($version -notmatch '^\d+\.\d+\.\d+$') { throw 'Expected a stable Tauri version.' }
$binary = Get-Item -LiteralPath $BinaryPath
$reader = [IO.BinaryReader]::new($binary.OpenRead())
try {
    if ($reader.ReadUInt16() -ne 0x5A4D) { throw 'Expected a Windows executable.' }
    $reader.BaseStream.Position = 0x3c
    $reader.BaseStream.Position = $reader.ReadUInt32()
    if ($reader.ReadUInt32() -ne 0x4550 -or $reader.ReadUInt16() -ne 0x8664) {
        throw 'Expected an x64 Windows executable.'
    }
} finally {
    $reader.Dispose()
}

[IO.Directory]::CreateDirectory($ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutputDirectory)) | Out-Null
$output = (Resolve-Path -LiteralPath $OutputDirectory).Path
$archive = Join-Path $output "MarketFloat-Tauri-$version-x64-portable.zip"
$temporaryZip = Join-Path $output ('.portable-' + [guid]::NewGuid().ToString('N') + '.zip')
Add-Type -AssemblyName System.IO.Compression.FileSystem
try {
    $zip = [IO.Compression.ZipFile]::Open($temporaryZip, [IO.Compression.ZipArchiveMode]::Create)
    try {
        [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip, $binary.FullName, 'MarketFloat-Tauri.exe') | Out-Null
        [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip, (Join-Path $root 'README.md'), 'README.md') | Out-Null
    } finally {
        $zip.Dispose()
    }
    [IO.File]::Move($temporaryZip, $archive, $true)
} finally {
    if (Test-Path -LiteralPath $temporaryZip) { Remove-Item -LiteralPath $temporaryZip }
}
$archive
