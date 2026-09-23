param(
  [string]$CMake = "cmake",
  [string]$SourceArchive = "",
  [string]$InstallDirectory = "$PSScriptRoot/artifacts"
)
$ErrorActionPreference = "Stop"
$source = Join-Path $PSScriptRoot "usvfs"
$revision = "57f1ea5e6ad13f7435a7af184748e6c1312c5637"
if ($SourceArchive) {
  $manifest = Get-Content -Raw (Join-Path $PSScriptRoot "usvfs-source.json") | ConvertFrom-Json
  if ($manifest.revision -ne $revision) { throw "Wrong archive source revision" }
  $actual = (Get-FileHash -Algorithm SHA256 $SourceArchive).Hash
  if ($actual -ne $manifest.sha256) { throw "Upstream source archive checksum mismatch" }
  if (Test-Path $source) {
    if (Get-ChildItem -Force $source | Select-Object -First 1) { throw "Archive builds require an empty native/usvfs directory; existing source is never replaced" }
  } else {
    New-Item -ItemType Directory $source | Out-Null
  }
  & tar.exe -xf $SourceArchive --strip-components=1 -C $source
  if ($LASTEXITCODE -ne 0) { throw "Verified source archive extraction failed" }
} else {
  if ((git -C $source rev-parse HEAD) -ne $revision) { throw "Wrong usvfs source revision" }
  git -C $source diff --quiet HEAD --
  if ($LASTEXITCODE -ne 0) { throw "usvfs source must be unmodified" }
  if (git -C $source ls-files --others --exclude-standard) { throw "usvfs contains untracked source files" }
}
if (-not $env:VCPKG_ROOT) { throw "Set VCPKG_ROOT before building upstream" }
$InstallDirectory = [System.IO.Path]::GetFullPath($InstallDirectory)
foreach ($arch in @("x86", "x64")) {
  Push-Location $source
  try {
    & $CMake --preset "vs2022-windows-$arch" -B "build_mods_$arch" -DBUILD_TESTING=OFF "-DCMAKE_INSTALL_PREFIX=$InstallDirectory"
    if ($LASTEXITCODE -ne 0) { throw "Upstream $arch configure failed" }
    & $CMake --build "build_mods_$arch" --config Release --target INSTALL
    if ($LASTEXITCODE -ne 0) { throw "Upstream $arch build failed" }
  } finally { Pop-Location }
}
$bin = Join-Path $InstallDirectory "bin"
Set-Content -NoNewline -Encoding ascii (Join-Path $bin "source-revision.txt") $revision
$files = @("usvfs_x86.dll", "usvfs_x64.dll", "usvfs_proxy_x86.exe", "usvfs_proxy_x64.exe")
$hashes = [ordered]@{}
foreach ($name in $files) {
  $hashes[$name] = (Get-FileHash -Algorithm SHA256 (Join-Path $bin $name)).Hash.ToLowerInvariant()
}
@{ source = $revision; configuration = "Release"; artifacts = $hashes } |
  ConvertTo-Json | Set-Content -Encoding ascii (Join-Path $bin "artifacts.json")
Write-Output "MODS_USVFS_ARTIFACTS=$bin"
