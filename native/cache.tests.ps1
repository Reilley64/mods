# Offline tests. Run with: pwsh -NoProfile -File native/cache.tests.ps1
$ErrorActionPreference = "Stop"
$helper = Join-Path $PSScriptRoot "cache.ps1"
$revision = (Get-Content -Raw (Join-Path $PSScriptRoot "usvfs-source.json") | ConvertFrom-Json).revision
$temporary = Join-Path ([System.IO.Path]::GetTempPath()) ([guid]::NewGuid().ToString())
$bin = Join-Path $temporary "bin"
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
  try { & $helper -Mode Validate -InstallDirectory $temporary | Out-Null } catch { $rejected = $true }
  if (-not $rejected) { throw "Expected rejection: $Name" }
  Write-Output "PASS: $Name"
}
try {
  New-TestBundle
  & $helper -Mode Validate -InstallDirectory $temporary | Out-Null
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
} finally {
  if (Test-Path $temporary) { Remove-Item -Recurse -Force $temporary }
}
