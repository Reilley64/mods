param(
  [ValidateSet("Fetch", "Validate")]
  [string]$Mode = "Fetch",
  [string]$ManifestPath = "$PSScriptRoot/usvfs-release.json",
  [string]$InstallDirectory = "$PSScriptRoot/artifacts",
  [string]$BundleArchive
)
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
if (-not (Test-Path -LiteralPath $ManifestPath -PathType Leaf)) {
  throw "Missing release manifest: $ManifestPath. Pin a published release and its verified SHA256 values first."
}
$manifest = Get-Content -Raw -LiteralPath $ManifestPath | ConvertFrom-Json
if ($manifest.schema -ne 1 -or $manifest.repository -cne "Reilley64/usvfs-rs") {
  throw "Unsupported native release manifest"
}
if ($manifest.tag -cnotmatch '^usvfs-[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+-rs\.[0-9]+$') {
  throw "A versioned native release tag is required"
}
$revision = "57f1ea5e6ad13f7435a7af184748e6c1312c5637"
if ($manifest.sourceRevision -cne $revision -or $manifest.forkRevision -cnotmatch '^[0-9a-f]{40}$') {
  throw "Invalid pinned source or fork revision"
}
foreach ($field in @("bundleSha256", "sourceSha256")) {
  if ($manifest.$field -cnotmatch '^[0-9a-f]{64}$') { throw "Invalid SHA256: $field" }
}
foreach ($field in @("bundleAsset", "sourceAsset")) {
  if ($manifest.$field -cnotmatch '^[A-Za-z0-9][A-Za-z0-9._-]*$') { throw "Invalid asset name: $field" }
}
if (-not $manifest.bundleAsset.EndsWith(".zip", [StringComparison]::Ordinal)) {
  throw "Native bundle must be a ZIP asset"
}

function Test-NativeBundle {
  param([string]$Directory)
  $bin = Join-Path $Directory "bin"
  $marker = Get-Content -Raw -LiteralPath (Join-Path $bin "source-revision.txt")
  if ($marker -cne $revision) { throw "Wrong bundle source revision" }
  $metadata = Get-Content -Raw -LiteralPath (Join-Path $bin "artifacts.json") | ConvertFrom-Json -AsHashtable
  if ($metadata.source -cne $revision -or $metadata.configuration -cne "Release") {
    throw "Wrong bundle source or configuration"
  }
  $names = @("usvfs_x86.dll", "usvfs_x64.dll", "usvfs_proxy_x86.exe", "usvfs_proxy_x64.exe")
  if ($metadata.artifacts -isnot [System.Collections.IDictionary] -or $metadata.artifacts.Count -ne $names.Count) {
    throw "Bundle must describe exactly the four required artifacts"
  }
  foreach ($name in $names) {
    if ($name -cnotin @($metadata.artifacts.Keys)) { throw "Missing artifact entry: $name" }
    $expected = $metadata.artifacts[$name]
    if ($expected -isnot [string] -or $expected -notmatch '^[0-9a-fA-F]{64}$') {
      throw "Invalid SHA256 for $name"
    }
    $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $bin $name)).Hash
    if ($actual -ne $expected) { throw "Artifact checksum mismatch: $name" }
  }
}

if ($Mode -eq "Validate") {
  Test-NativeBundle $InstallDirectory
  Write-Output "Native artifact bundle verified"
  return
}
$destination = [System.IO.Path]::GetFullPath($InstallDirectory)
if (Test-Path -LiteralPath $destination) { throw "Native destination already exists: $destination. Choose a new destination; existing artifacts are never overwritten." }
$parent = Split-Path -Parent $destination
New-Item -ItemType Directory -Force $parent | Out-Null
$temporary = Join-Path $parent (".usvfs-fetch-" + [guid]::NewGuid().ToString())
New-Item -ItemType Directory $temporary | Out-Null
try {
  $archive = Join-Path $temporary "bundle.zip"
  if ($BundleArchive) {
    Copy-Item -LiteralPath $BundleArchive -Destination $archive
  } else {
    $url = "https://github.com/$($manifest.repository)/releases/download/$($manifest.tag)/$($manifest.bundleAsset)"
    Invoke-WebRequest -Uri $url -OutFile $archive
  }
  if ((Get-FileHash -Algorithm SHA256 -LiteralPath $archive).Hash -ne $manifest.bundleSha256) {
    throw "Native release bundle checksum mismatch"
  }

  $staged = Join-Path $temporary "staged"
  Expand-Archive -LiteralPath $archive -DestinationPath $staged
  Test-NativeBundle $staged
  # Directory.Move fails if another process creates the destination during download.
  [System.IO.Directory]::Move($staged, $destination)
  Write-Output "Native artifact bundle verified: $destination/bin"
} finally {
  Remove-Item -LiteralPath $temporary -Recurse -Force
}
