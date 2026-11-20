param(
    [Parameter(Mandatory)][string] $Sdk,
    [Parameter(Mandatory)][string] $Jdk,
    [Parameter(Mandatory)][string] $Engine,
    [Parameter(Mandatory)][string] $Codec,
    [Parameter(Mandatory)][string] $CodecLicense,
    [Parameter(Mandatory)][string] $Keystore,
    [Parameter(Mandatory)][string] $Output
)
$ErrorActionPreference = 'Stop'
$env:JAVA_HOME = (Resolve-Path -LiteralPath $Jdk).Path
$Sdk = (Resolve-Path -LiteralPath $Sdk).Path
$build = Join-Path $Sdk 'build-tools/35.0.0'
$api = Join-Path $Sdk 'platforms/android-35/android.jar'
$ndk = Join-Path $Sdk 'ndk/27.3.13750724/toolchains/llvm/prebuilt/windows-x86_64'
$libcpp = Join-Path $ndk 'sysroot/usr/lib/aarch64-linux-android/libc++_shared.so'
$work = (New-Item -ItemType Directory -Force -Path "${Output}.work").FullName
$classes = (New-Item -ItemType Directory -Force -Path "$work/classes").FullName
$payload = (New-Item -ItemType Directory -Force -Path "$work/dex").FullName
function Invoke-Tool([string] $Tool, [string[]] $Arguments) {
    & $Tool @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Tool failed with exit $LASTEXITCODE" }
}
function Copy-ArmLibrary([string] $Path, [string] $Name) {
    $result = & "$ndk/bin/llvm-readelf.exe" -lW $Path
    if ($LASTEXITCODE -ne 0) { throw "Cannot inspect $Name" }
    $segments = @($result | Where-Object { $_ -match '^\s*LOAD\s' })
    if (!$segments.Count -or @($segments | Where-Object { $_ -notmatch '\s0x(?:4000|10000)\s*$' }).Count) {
        throw "$Name has a load segment without 16 KiB alignment"
    }
    Invoke-Tool "$ndk/bin/llvm-strip.exe" @('--strip-unneeded', '-o', "$work/$Name", $Path)
}
Copy-ArmLibrary $Engine 'libblotworx.so'
Copy-ArmLibrary $Codec 'libbwx_ffmpeg.so'
Copy-ArmLibrary $libcpp 'libc++_shared.so'
$symbols = & "$ndk/bin/llvm-readelf.exe" --wide --dyn-syms "$work/libblotworx.so"
foreach ($symbol in 'ANativeActivity_onCreate','Java_dev_bloticle_blotworx_BlotworxActivity_updateGamepad') {
    if (!(($symbols -join "`n") -match "\b$symbol\b")) { throw "Missing native entry point $symbol" }
}
$sources = @(Get-ChildItem -LiteralPath $PSScriptRoot -Filter '*.java' -File | Select-Object -ExpandProperty FullName)
Invoke-Tool "$env:JAVA_HOME/bin/javac.exe" (@('-Xlint:-options','--release','8','-cp',$api,'-d',$classes) + $sources)
$bytecode = @(Get-ChildItem -LiteralPath $classes -Recurse -Filter '*.class' -File | Select-Object -ExpandProperty FullName)
Invoke-Tool "$build/d8.bat" (@('--min-api','26','--lib',$api,'--output',$payload) + $bytecode)
Invoke-Tool "$build/aapt2.exe" @('link','-I',$api,'--manifest',"$PSScriptRoot/AndroidManifest.xml",'-o',"$work/unsigned.apk")
Copy-Item -LiteralPath "$PSScriptRoot/../LICENSE" -Destination "$work/Blotworx-GPL-3.txt" -Force
Copy-Item -LiteralPath $CodecLicense -Destination "$work/FFmpeg-LGPL-2.1.txt" -Force
Copy-Item -LiteralPath "$PSScriptRoot/README.md" -Destination "$work/Android.md" -Force
$zip = @"
import sys, zipfile
from pathlib import Path
work = Path(sys.argv[1])
with zipfile.ZipFile(work / 'unsigned.apk', 'a') as apk:
    apk.write(work / 'dex/classes.dex', 'classes.dex')
    for source in work.glob('*.so'):
        apk.write(source, f'lib/arm64-v8a/{source.name}')
    for name in ('Blotworx-GPL-3.txt', 'FFmpeg-LGPL-2.1.txt', 'Android.md'):
        apk.write(work / name, f'assets/notices/{name}', compress_type=zipfile.ZIP_DEFLATED)
"@
Invoke-Tool 'python' @('-c',$zip,$work)
Invoke-Tool "$build/zipalign.exe" @('-f','-P','16','4',"$work/unsigned.apk","$work/aligned.apk")
Invoke-Tool "$build/apksigner.bat" @('sign','--ks',$Keystore,'--ks-key-alias','blotworx','--ks-pass','env:BLOTWORX_SIGNING_PASSWORD','--out',$Output,"$work/aligned.apk")
Invoke-Tool "$build/apksigner.bat" @('verify','--verbose','--print-certs',$Output)
Invoke-Tool "$build/zipalign.exe" @('-c','-P','16','4',$Output)
Invoke-Tool "$build/aapt2.exe" @('dump','badging',$Output)
(Get-FileHash -LiteralPath $Output -Algorithm SHA256).Hash.ToLowerInvariant() + '  ' + (Split-Path $Output -Leaf) |
    Set-Content -Encoding ascii "${Output}.sha256"
