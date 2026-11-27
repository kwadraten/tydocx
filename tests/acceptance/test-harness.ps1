param(
    [string]$BuildLog,
    [switch]$Visual
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$toolchain = Join-Path $env:USERPROFILE '.rustup\toolchains\1.98.0-x86_64-pc-windows-msvc\bin'
$deps = Join-Path $root 'target\debug\deps'
$out = Join-Path $root 'output\render-trials'
New-Item -ItemType Directory -Path $out -Force | Out-Null
$exe = Join-Path $out 'render-corpus-tests.exe'
$source = Join-Path $PSScriptRoot 'test_harness.rs'
$rustArgs = @('--edition=2024', '--test', $source, '-L', "dependency=$deps", '-o', $exe)
if ($BuildLog) {
    $invocation = Get-Content $BuildLog | Where-Object { $_ -match '--crate-name integration ' } |
        Select-Object -Last 1
    if (!$invocation) { throw "need a cargo -vv log that compiled the integration target" }
    foreach ($match in [regex]::Matches($invocation, '--extern (\S+\.rlib)|-L (native=\S+)')) {
        if ($match.Groups[1].Success) { $rustArgs += @('--extern', $match.Groups[1].Value) }
        else { $rustArgs += @('-L', $match.Groups[2].Value.TrimEnd('`')) }
    }
} else {
    $artifacts = @('tydocx', 'typst', 'typst_layout', 'typst_pdf', 'zip')
    foreach ($crate in $artifacts) {
        $artifact = Get-ChildItem $deps -Filter "lib${crate}-*.rlib" |
            Sort-Object LastWriteTime -Descending | Select-Object -First 1
        if (!$artifact) { throw "artifact for $crate absent; use -BuildLog" }
        $rustArgs += @('--extern', "$crate=$($artifact.FullName)")
    }
    foreach ($path in 'stacker-d14fe15233a6b2c6', 'psm-f8159a02719aad32') {
        $rustArgs += @('-L', "native=$root\target\debug\build\$path\out")
    }
}
$env:CARGO_MANIFEST_DIR = $root
& "$toolchain\rustc.exe" @rustArgs
if ($LASTEXITCODE) { exit $LASTEXITCODE }
Push-Location $root
try {
    if ($Visual) { & $exe --include-ignored --test-threads=1 }
    else { & $exe --test-threads=4 }
    exit $LASTEXITCODE
} finally {
    Pop-Location
}
