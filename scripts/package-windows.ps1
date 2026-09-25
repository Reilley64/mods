# Build and package a clean committed source snapshot. Publication is deliberately separate.
param(
  [Parameter(Mandatory)][ValidatePattern('^(?:[0-9a-f]{40}|v[0-9]+\.[0-9]+\.[0-9]+)$')]
  [string]$ReleaseId,
  [string]$OutputDirectory = "$PSScriptRoot/../dist",
  [string]$BundleArchive,
  [string]$SourceArchive
)
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
if (-not $IsWindows) { throw "Packaging requires Windows and PowerShell 7" }
$root = (Resolve-Path "$PSScriptRoot/..").Path
$output = [IO.Path]::GetFullPath($OutputDirectory)
if (Test-Path -LiteralPath $output) { throw "Output directory already exists: $output" }
# Absolute paths survive the move into the staged source directory.
if ($BundleArchive) { $BundleArchive = (Resolve-Path -LiteralPath $BundleArchive).Path }
if ($SourceArchive) { $SourceArchive = (Resolve-Path -LiteralPath $SourceArchive).Path }
# Keep staging on the output volume so the final directory move is atomic.
$outputParent = Split-Path -Parent $output
New-Item -ItemType Directory -Force $outputParent | Out-Null
$temporary = Join-Path $outputParent (".mods-package-" + [guid]::NewGuid())
$previousArtifacts = $env:MODS_USVFS_ARTIFACTS
$previousTarget = $env:CARGO_TARGET_DIR
Push-Location $root
try {
  $revision = git rev-parse HEAD
  if ($LASTEXITCODE -ne 0) { throw "Cannot resolve source revision" }
  if ($ReleaseId.StartsWith("v")) {
    $tagRevision = git rev-parse "$ReleaseId^{commit}"
    if ($LASTEXITCODE -ne 0 -or $tagRevision -cne $revision) { throw "Stable tag must identify HEAD" }
    if ((Get-Content -Raw version.txt).Trim() -cne $ReleaseId.Substring(1) -or $ReleaseId -eq "v0.0.0") {
      throw "Stable tag must match the nonzero aggregate version"
    }
  } elseif ($ReleaseId -cne $revision) { throw "Preview ID must be the full HEAD SHA" }
  $dirty = git status --porcelain --untracked-files=normal
  if ($LASTEXITCODE -ne 0 -or $dirty) { throw "Package only a clean committed checkout" }
  New-Item -ItemType Directory $temporary | Out-Null
  $snapshot = Join-Path $temporary "mods.tar"
  git archive --format=tar "--output=$snapshot" HEAD
  if ($LASTEXITCODE -ne 0) { throw "Source archive failed" }
  $source = Join-Path $temporary "source"
  $package = Join-Path $temporary "package"
  $result = Join-Path $temporary "result"
  New-Item -ItemType Directory $source, $package, $result | Out-Null
  tar -xf $snapshot -C $source
  if ($LASTEXITCODE -ne 0) { throw "Source extraction failed" }
  Set-Location $source
  $release = Get-Content -Raw native/usvfs-release.json | ConvertFrom-Json
  New-Item -ItemType Directory native/archives | Out-Null
  $bundle = Join-Path $source "native/archives/$($release.bundleAsset)"
  if ($BundleArchive) { Copy-Item -LiteralPath $BundleArchive $bundle }
  else { Invoke-WebRequest "https://github.com/$($release.repository)/releases/download/$($release.tag)/$($release.bundleAsset)" -OutFile $bundle }
  # Reuse the native manifest, ZIP hash, source marker and exact four-artifact policy.
  ./native/fetch.ps1 -BundleArchive $bundle
  $nativeSource = Join-Path $source "native/archives/$($release.sourceAsset)"
  if ($SourceArchive) { Copy-Item -LiteralPath $SourceArchive $nativeSource }
  else { Invoke-WebRequest "https://github.com/$($release.repository)/releases/download/$($release.tag)/$($release.sourceAsset)" -OutFile $nativeSource }
  if ((Get-FileHash -Algorithm SHA256 -LiteralPath $nativeSource).Hash -ne $release.sourceSha256) {
    throw "Native corresponding-source checksum mismatch"
  }
  # cargo vendor omits the fork-root headers used by usvfs-sys/../../include.
  # Extract only the required tree from the already hash-verified native source.
  $nativeCheckout = Join-Path $temporary "native-checkout"
  New-Item -ItemType Directory $nativeCheckout | Out-Null
  tar -xf $nativeSource -C $nativeCheckout source-candidate/checkouts/fork.tar.gz
  if ($LASTEXITCODE -ne 0) { throw "Native fork archive extraction failed" }
  $forkArchive = Join-Path $nativeCheckout "source-candidate/checkouts/fork.tar.gz"
  tar -xf $forkArchive -C $source include
  if ($LASTEXITCODE -ne 0) { throw "Native header extraction failed" }
  if (-not (Test-Path include/usvfs/usvfs.h -PathType Leaf)) { throw "Pinned native headers are required" }
  if (-not (Test-Path native/artifacts/licenses -PathType Container)) { throw "Native release notices are required" }
  New-Item -ItemType Directory .cargo | Out-Null
  # Cargo emits source replacement configuration for locked registry AND Git sources.
  $config = cargo vendor --locked --versioned-dirs vendor
  if ($LASTEXITCODE -ne 0) { throw "Locked dependency vendoring failed" }
  $config | Set-Content -Encoding utf8NoBOM .cargo/config.toml
  Copy-Item docs/distribution.md OFFLINE-REBUILD.md
  $revision | Set-Content -Encoding utf8NoBOM SOURCE-REVISION.txt
  $env:MODS_USVFS_ARTIFACTS = Join-Path $source "native/artifacts/bin"
  $env:CARGO_TARGET_DIR = Join-Path $temporary "target"
  cargo build --release --frozen --target x86_64-pc-windows-msvc --package mods --package mods-mcp --bins
  if ($LASTEXITCODE -ne 0) { throw "Vendored offline Cargo build failed" }
  foreach ($binary in @("mods.exe", "mods-mcp.exe")) {
    Copy-Item -LiteralPath "$env:CARGO_TARGET_DIR/x86_64-pc-windows-msvc/release/$binary" $package
  }
  New-Item -ItemType Directory "$package/usvfs", "$package/licenses" | Out-Null
  foreach ($artifact in @("usvfs_x86.dll", "usvfs_x64.dll", "usvfs_proxy_x86.exe", "usvfs_proxy_x64.exe")) {
    Copy-Item -LiteralPath "native/artifacts/bin/$artifact" "$package/usvfs"
  }
  Copy-Item LICENSE, COPYRIGHT.md $package
  Copy-Item docs/distribution.md "$package/BUILD-AND-SOURCE.md"
  Copy-Item -Recurse licenses/usvfs "$package/licenses"
  Copy-Item -Recurse native/artifacts/licenses "$package/licenses/native-release"
  # Preserve all vendored crate license/readme files in the corresponding source.
  # The hashed native ZIP remains available for a fresh offline fetch.
  Remove-Item -Recurse -Force native/artifacts
  tar -czf "$package/mods-source.tar.gz" -C $source .
  if ($LASTEXITCODE -ne 0) { throw "Corresponding-source archive failed" }
  $name = "mods-$ReleaseId-x86_64-pc-windows-msvc.zip"
  # tar's ZIP mode includes dotfiles; Compress-Archive omits hidden files.
  tar -a -cf "$result/$name" -C $package .
  if ($LASTEXITCODE -ne 0) { throw "Distribution ZIP failed" }
  $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath "$result/$name").Hash.ToLowerInvariant()
  "$hash  $name" | Set-Content -Encoding ascii "$result/SHA256SUMS"
  New-Item -ItemType Directory -Force (Split-Path -Parent $output) | Out-Null
  [IO.Directory]::Move($result, $output)
  Write-Output "Candidate distribution: $output (publication remains gated)"
} finally {
  Pop-Location
  $env:MODS_USVFS_ARTIFACTS = $previousArtifacts
  $env:CARGO_TARGET_DIR = $previousTarget
  if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary -Recurse -Force }
}
