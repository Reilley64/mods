param(
  [Parameter(Mandatory)]
  [ValidateSet("Key", "Validate")]
  [string]$Mode,
  [string]$InstallDirectory = "$PSScriptRoot/artifacts"
)
$ErrorActionPreference = "Stop"
$manifestPath = Join-Path $PSScriptRoot "usvfs-source.json"
$manifest = Get-Content -Raw $manifestPath | ConvertFrom-Json
if ($manifest.revision -cnotmatch '^[0-9a-f]{40}$') { throw "Invalid pinned source revision" }
$revision = $manifest.revision

if ($Mode -eq "Validate") {
  $bin = Join-Path $InstallDirectory "bin"
  $marker = Get-Content -Raw (Join-Path $bin "source-revision.txt")
  if ($marker -cne $revision) { throw "Wrong bundle source revision" }
  $metadata = Get-Content -Raw (Join-Path $bin "artifacts.json") | ConvertFrom-Json -AsHashtable
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
    $actual = (Get-FileHash -Algorithm SHA256 (Join-Path $bin $name)).Hash
    if ($actual -ne $expected) { throw "Artifact checksum mismatch: $name" }
  }
  Write-Output "Native artifact bundle verified"
  return
}

function Invoke-Checked {
  param([string]$Command, [string[]]$Arguments)
  $result = & $Command @Arguments
  if ($LASTEXITCODE -ne 0) { throw "$Command failed with exit code $LASTEXITCODE" }
  return $result
}
function Get-Fingerprint {
  param($Value)
  $json = ConvertTo-Json -InputObject $Value -Depth 20 -Compress
  $bytes = [System.Text.Encoding]::UTF8.GetBytes($json)
  return [Convert]::ToHexString([System.Security.Cryptography.SHA256]::HashData($bytes)).ToLowerInvariant()
}
$source = Join-Path $PSScriptRoot "usvfs"
$head = Invoke-Checked git @("-C", $source, "rev-parse", "HEAD")
if ($head -cne $revision) { throw "Wrong usvfs source revision" }
Invoke-Checked git @("-C", $source, "diff", "--quiet", "HEAD", "--")
$untracked = Invoke-Checked git @("-C", $source, "ls-files", "--others", "--exclude-standard")
if ($untracked) { throw "usvfs contains untracked source files" }
if (-not $IsWindows) { throw "Key mode requires the Windows build toolchain" }
if (-not $env:VCPKG_ROOT) { throw "Set VCPKG_ROOT before computing cache keys" }
if (-not $env:ImageOS -or -not $env:ImageVersion) { throw "Runner ImageOS and ImageVersion are required" }
$vcpkgRevision = Invoke-Checked git @("-C", $env:VCPKG_ROOT, "rev-parse", "HEAD")
$vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio/Installer/vswhere.exe"
$installations = @(Invoke-Checked $vswhere @("-version", "[17.0,18.0)", "-products", "*", "-requires", "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-format", "json", "-utf8") | Out-String | ConvertFrom-Json)
$visualStudio = @(foreach ($installation in ($installations | Sort-Object installationVersion, installationPath)) {
  $tools = Join-Path $installation.installationPath "VC/Tools/MSVC"
  $toolsets = @(foreach ($toolset in (Get-ChildItem -Directory $tools | Sort-Object Name)) {
    $compilers = @(foreach ($relative in @("bin/Hostx64/x86/cl.exe", "bin/Hostx64/x64/cl.exe", "bin/Hostx86/x86/cl.exe", "bin/Hostx86/x64/cl.exe")) {
      $compiler = Join-Path $toolset.FullName $relative
      if (Test-Path -LiteralPath $compiler -PathType Leaf) {
        [ordered]@{ path = $relative; version = (Get-Item -LiteralPath $compiler).VersionInfo.FileVersion; sha256 = (Get-FileHash -Algorithm SHA256 $compiler).Hash }
      }
    })
    [ordered]@{ version = $toolset.Name; compilers = $compilers }
  })
  $defaults = @(Get-ChildItem (Join-Path $installation.installationPath "VC/Auxiliary/Build") -Filter "Microsoft.VCToolsVersion*.txt" | Sort-Object Name | ForEach-Object {
    [ordered]@{ name = $_.Name; value = (Get-Content -Raw $_.FullName).Trim() }
  })
  [ordered]@{ version = $installation.installationVersion; product = $installation.productId; toolsets = $toolsets; defaults = $defaults }
})
if ($visualStudio.Count -eq 0) { throw "No Visual Studio 2022 C++ installation found" }
$kits = Get-ItemProperty "HKLM:/SOFTWARE/Microsoft/Windows Kits/Installed Roots"
$sdkRoot = $kits.KitsRoot10
if (-not $sdkRoot) { throw "Windows SDK root not found" }
$sdkInventory = [ordered]@{}
foreach ($part in @("Include", "Lib", "bin")) {
  $sdkInventory[$part] = @(Get-ChildItem -Directory (Join-Path $sdkRoot $part) | Where-Object Name -Match '^10\.' | Sort-Object Name | Select-Object -ExpandProperty Name)
  if ($sdkInventory[$part].Count -eq 0) { throw "Windows SDK $part inventory is empty" }
}
$cmakeVersion = @(Invoke-Checked cmake @("--version"))
$dependencies = [ordered]@{
  schema = 1
  architectures = @("x86", "x64")
  configuration = "Release"
  buildTesting = "OFF"
  generator = "Visual Studio 17 2022"
  imageOS = $env:ImageOS
  imageVersion = $env:ImageVersion
  visualStudio = $visualStudio
  windowsSDKs = $sdkInventory
  cmake = $cmakeVersion
  vcpkgRevision = $vcpkgRevision
  upstreamRevision = $revision
}
$dependencyKey = "native-dependencies-v1-$(Get-Fingerprint $dependencies)"
$bundle = [ordered]@{
  dependencies = $dependencies
  manifest = (Get-FileHash -Algorithm SHA256 $manifestPath).Hash
  buildScript = (Get-FileHash -Algorithm SHA256 (Join-Path $PSScriptRoot "build.ps1")).Hash
  cacheScript = (Get-FileHash -Algorithm SHA256 $PSCommandPath).Hash
}
$bundleKey = "native-bundle-v1-$(Get-Fingerprint $bundle)"
$outputLines = @("bundle-key=$bundleKey", "dependency-key=$dependencyKey")
if ($env:GITHUB_OUTPUT) { $outputLines | Add-Content -Encoding utf8 $env:GITHUB_OUTPUT }
$outputLines | Write-Output
