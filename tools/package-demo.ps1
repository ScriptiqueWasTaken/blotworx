param(
    [Parameter(Mandatory)][string] $Executable,
    [Parameter(Mandatory)][string] $OutputDirectory
)

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$version = [regex]::Match(
    (Get-Content (Join-Path $root 'Cargo.toml') -Raw), '(?m)^version = "([^"]+)"'
).Groups[1].Value
if (!$version) { throw 'Blotworx package version is missing.' }
$executablePath = (Resolve-Path -LiteralPath $Executable).Path
$output = (New-Item -ItemType Directory -Force -Path $OutputDirectory).FullName
$stage = (New-Item -ItemType Directory -Path (Join-Path $output "Blotworx-$version")).FullName
$updaterAsset = Join-Path $output 'Blotworx-Windows-x64.exe'
$archive = Join-Path $output "Blotworx-$version-Windows-x64.zip"

Copy-Item -LiteralPath $executablePath -Destination (Join-Path $stage 'Blotworx.exe')
Copy-Item -LiteralPath $executablePath -Destination $updaterAsset
Copy-Item -LiteralPath (Join-Path $root 'LICENSE') -Destination $stage
Copy-Item -LiteralPath (Join-Path $root 'docs/PUBLIC_DEMO.md') -Destination (Join-Path $stage 'Getting Started.md')
Copy-Item -LiteralPath (Join-Path $root 'third_party/README.md') -Destination (Join-Path $stage 'Third-party sources.md')

$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
$installation = & $vswhere -latest -products '*' -property installationPath
if (!$installation) { throw 'Visual Studio Build Tools are required to find the app-local runtime.' }
$redistRoot = Join-Path $installation 'VC/Redist/MSVC'
$runtime = Get-ChildItem -LiteralPath $redistRoot -Directory |
    Where-Object Name -Match '^\d+\.\d+\.\d+$' |
    Sort-Object { [version]$_.Name } -Descending |
    ForEach-Object { Get-ChildItem -Path (Join-Path $_.FullName 'x64/Microsoft.VC*.CRT') -Directory -ErrorAction SilentlyContinue } |
    Select-Object -First 1
if (!$runtime) { throw 'The x64 Visual C++ redistributable files are missing.' }
foreach ($name in 'msvcp140.dll','vcruntime140.dll','vcruntime140_1.dll') {
    Copy-Item -LiteralPath (Join-Path $runtime.FullName $name) -Destination $stage
}

Compress-Archive -LiteralPath $stage -DestinationPath $archive -CompressionLevel Optimal
Get-FileHash -Algorithm SHA256 -LiteralPath $archive,$updaterAsset | ForEach-Object {
    '{0}  {1}' -f $_.Hash.ToLowerInvariant(), (Split-Path $_.Path -Leaf)
} | Set-Content (Join-Path $output 'SHA256SUMS.txt') -Encoding ascii
