# Offline tests. Run with: pwsh -NoProfile -File native/fetch.tests.ps1
$ErrorActionPreference = "Stop"
$helper = Join-Path $PSScriptRoot "fetch.ps1"
$revision = (Get-Content -Raw (Join-Path $PSScriptRoot "usvfs-source.json") | ConvertFrom-Json).revision
$temporary = Join-Path ([System.IO.Path]::GetTempPath()) ([guid]::NewGuid().ToString())
$bundle = Join-Path $temporary "bundle"
$bin = Join-Path $bundle "bin"
$manifestPath = Join-Path $temporary "release.json"
$names = @("usvfs_x86.dll", "usvfs_x64.dll", "usvfs_proxy_x86.exe", "usvfs_proxy_x64.exe")
function New-TestBundle {
  New-Item -ItemType Directory -Force $bin | Out-Null
  $hashes = [ordered]@{}
  foreach ($name in $names) {
    Set-Content -LiteralPath (Join-Path $bin $name) -Value "fixture-$name" -NoNewline -Encoding ascii
    $hashes[$name] = (Get-FileHash -Algorithm SHA256 (Join-Path $bin $name)).Hash.ToLowerInvariant()
  }
  Set-Content -LiteralPath (Join-Path $bin "source-revision.txt") -Value $revision -NoNewline -Encoding ascii
  $script:metadata = @{ source = $revision; configuration = "Release"; artifacts = $hashes }
  Save-Metadata
}
function Save-Metadata {
  $script:metadata | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $bin "artifacts.json")
}
function Assert-Rejected {
  param([string]$Name)
  $rejected = $false
  try { & $helper -Mode Validate -ManifestPath $manifestPath -InstallDirectory $bundle | Out-Null } catch { $rejected = $true }
  if (-not $rejected) { throw "Expected rejection: $Name" }
  Write-Output "PASS: $Name"
}
try {
  New-Item -ItemType Directory $temporary | Out-Null
  $release = @{
    schema = 1; repository = "Reilley64/usvfs-rs"; tag = "usvfs-0.5.7.2-rs.1"
    bundleAsset = "fixture.zip"; bundleSha256 = "a" * 64
    sourceRevision = $revision; sourceAsset = "fixture-source.tar.gz"
    sourceSha256 = "b" * 64; forkRevision = "c" * 40
  }
  $release | ConvertTo-Json | Set-Content -LiteralPath $manifestPath
  New-TestBundle
  & $helper -Mode Validate -ManifestPath $manifestPath -InstallDirectory $bundle | Out-Null
  Write-Output "PASS: valid bundle"
  Add-Content (Join-Path $bin $names[0]) "tampered"
  Assert-Rejected "tampered content"
  New-TestBundle
  Remove-Item (Join-Path $bin $names[1])
  Assert-Rejected "missing file"
  New-TestBundle
  Set-Content -LiteralPath (Join-Path $bin "source-revision.txt") -Value ("0" * 40) -NoNewline
  Assert-Rejected "wrong revision marker"
  New-TestBundle
  $metadata.source = "0" * 40
  Save-Metadata
  Assert-Rejected "wrong metadata revision"
  New-TestBundle
  $metadata.configuration = "Debug"
  Save-Metadata
  Assert-Rejected "wrong configuration"
  New-TestBundle
  $metadata.artifacts[$names[0]] = "not-a-sha256"
  Save-Metadata
  Assert-Rejected "malformed hash"
  New-TestBundle
  $metadata.artifacts.Remove($names[0])
  Save-Metadata
  Assert-Rejected "missing entry"
  New-TestBundle
  $metadata.artifacts["extra.dll"] = "a" * 64
  Save-Metadata
  Assert-Rejected "extra entry"
  New-TestBundle
  Set-Content -LiteralPath (Join-Path $bin "artifacts.json") -Value "not-json"
  Assert-Rejected "invalid metadata JSON"
  New-TestBundle
  $archive = Join-Path $temporary "fixture.zip"
  Compress-Archive -Path $bin -DestinationPath $archive
  $release.bundleSha256 = (Get-FileHash -Algorithm SHA256 $archive).Hash.ToLowerInvariant()
  $release | ConvertTo-Json | Set-Content -LiteralPath $manifestPath
  $destination = Join-Path $temporary "installed"
  & $helper -ManifestPath $manifestPath -InstallDirectory $destination -BundleArchive $archive | Out-Null
  if (-not (Test-Path (Join-Path $destination "bin/artifacts.json"))) { throw "Bundle was not installed" }
  Write-Output "PASS: verified archive installed"
  $rejected = $false
  try { & $helper -ManifestPath $manifestPath -InstallDirectory $destination -BundleArchive $archive | Out-Null } catch { $rejected = $true }
  if (-not $rejected) { throw "Existing destination was overwritten" }
  Write-Output "PASS: existing destination preserved"
  $destination = Join-Path $temporary "rejected"
  $release.bundleSha256 = "0" * 64
  $release | ConvertTo-Json | Set-Content -LiteralPath $manifestPath
  $rejected = $false
  try { & $helper -ManifestPath $manifestPath -InstallDirectory $destination -BundleArchive $archive | Out-Null } catch { $rejected = $true }
  if (-not $rejected -or (Test-Path $destination)) { throw "Invalid archive was installed" }
  if (@(Get-ChildItem $temporary -Force -Filter ".usvfs-fetch-*").Count -ne 0) { throw "Owned temporary files leaked" }
  Write-Output "PASS: checksum failure leaves no destination or temporary files"
  $release.tag = "latest"
  $release | ConvertTo-Json | Set-Content -LiteralPath $manifestPath
  Assert-Rejected "unpinned release tag"
  Remove-Item $manifestPath
  Assert-Rejected "missing release manifest"

} finally {
  if (Test-Path $temporary) { Remove-Item -Recurse -Force $temporary }
}
