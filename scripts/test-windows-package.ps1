# Run with pwsh -NoProfile -STA -File scripts/test-windows-package.ps1.
[CmdletBinding()]
param(
  [string]$Directory = "$PSScriptRoot/../dist",
  [string]$ReleaseId,
  [string]$ExpectedVersion = (Get-Content -Raw "$PSScriptRoot/../version.txt").Trim(),
  [switch]$Synthetic,
  [switch]$ExtractWorker,
  [string]$Archive,
  [string]$Destination
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if (-not $IsWindows -or $PSVersionTable.PSVersion.Major -lt 7) { throw 'Windows PowerShell 7 required' }
if ([Threading.Thread]::CurrentThread.GetApartmentState() -ne 'STA') { throw 'Restart pwsh with -STA' }
if ($ExtractWorker) {
  Add-Type -Path "$PSScriptRoot/windows-shell-extract.cs"
  $native = [ZipShellProbe]::Extract($Archive, $Destination)
  $native | ConvertTo-Json | Set-Content "$Destination/../native-result.json"
  if ($native.HResult -lt 0 -or $native.Aborted -eq $true -or $native.QueuedItems -eq 0) { throw 'Windows Shell extraction failed' }
  exit 0
}
. "$PSScriptRoot/windows-package.ps1"
$work = Join-Path ([IO.Path]::GetTempPath()) ("mods-package-smoke-" + [guid]::NewGuid())
$null = New-Item -ItemType Directory $work
Write-Host "Smoke evidence: $work (retained on failure)"
function Invoke-Bounded([string]$Program, [string[]]$Arguments, [string]$Label) {
  $info = [Diagnostics.ProcessStartInfo]::new($Program)
  $info.UseShellExecute = $false
  $info.RedirectStandardOutput = $true
  $info.RedirectStandardError = $true
  foreach ($argument in $Arguments) { $info.ArgumentList.Add($argument) }
  $process = [Diagnostics.Process]::Start($info)
  $stdout = $process.StandardOutput.ReadToEndAsync()
  $stderr = $process.StandardError.ReadToEndAsync()
  if (-not $process.WaitForExit(60000)) {
    "PID=$($process.Id)" | Set-Content "$work/$Label.timeout.txt"
    throw "$Label exceeded 60 seconds; process $($process.Id) left running for inspection; evidence: $work"
  }
  $out = $stdout.GetAwaiter().GetResult()
  $err = $stderr.GetAwaiter().GetResult()
  $out | Set-Content "$work/$Label.stdout.txt"
  $err | Set-Content "$work/$Label.stderr.txt"
  if ($process.ExitCode -ne 0) { throw "$Label failed ($($process.ExitCode))" }
  return $out
}
function Assert-Payload([string]$Zip, [string]$Target) {
  $expected = @{}
  $archiveReader = [IO.Compression.ZipFile]::OpenRead($Zip)
  try {
    foreach ($entry in $archiveReader.Entries) {
      $name = $entry.FullName
      if ($name -match '(^/|\\|(^|/)\.\.?(/|$))') { throw "Non-normalized ZIP path: $name" }
      if ($name.EndsWith('/')) {
        if (-not (Test-Path -LiteralPath (Join-Path $Target $name) -PathType Container)) { throw "Missing extracted directory: $name" }
        continue
      }
      if ($expected.ContainsKey($name)) { throw "Duplicate ZIP path: $name" }
      $stream = $entry.Open()
      try { $expected[$name] = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($stream)) }
      finally { $stream.Dispose() }
    }
  } finally { $archiveReader.Dispose() }
  if (-not $expected.Count) { throw 'Empty ZIP payload' }
  $files = @(Get-ChildItem -LiteralPath $Target -Recurse -File -Force)
  if ($files.Count -ne $expected.Count) { throw 'Extracted file count differs from ZIP' }
  foreach ($file in $files) {
    $relative = [IO.Path]::GetRelativePath($Target, $file.FullName).Replace('\', '/')
    if (-not $expected.ContainsKey($relative) -or (Get-FileHash -LiteralPath $file.FullName).Hash -cne $expected[$relative]) { throw "Extracted payload mismatch: $relative" }
  }
}
if ($Synthetic) {
  # This tests the production path formatter and Shell helper, not a release build.
  $fixture = Join-Path $work 'fixture'
  $null = New-Item -ItemType Directory "$fixture/usvfs", "$fixture/licenses"
  'synthetic runtime' | Set-Content "$fixture/mods.exe"
  'native fixture' | Set-Content "$fixture/usvfs/probe.dll"
  'notice fixture' | Set-Content "$fixture/licenses/NOTICE"
  'hidden fixture' | Set-Content "$fixture/.notice"
  $Archive = Join-Path $work 'normalized.zip'
  New-RuntimeZip -Directory $fixture -Archive $Archive
  $bad = Join-Path $work 'dot-prefix.zip'
  tar -a -cf $bad -C $fixture .
  if ($LASTEXITCODE -ne 0) { throw 'Negative fixture ZIP failed' }
  $rejected = $false
  try { Assert-Payload $bad $fixture } catch { if ($_ -notmatch 'Non-normalized ZIP path') { throw }; $rejected = $true }
  if (-not $rejected) { throw 'Dot-prefix regression was not rejected' }
} else {
  if ($ReleaseId -notmatch '^(v[0-9]+\.[0-9]+\.[0-9]+|[0-9a-f]{40})$') { throw 'ReleaseId required' }
  $Directory = (Resolve-Path -LiteralPath $Directory).Path
  $runtimeName = "mods-$ReleaseId-runtime-x86_64-pc-windows-msvc.zip"
  $sourceName = "mods-$ReleaseId-source.tar.gz"
  $lines = @(Get-Content -LiteralPath "$Directory/SHA256SUMS")
  if ($lines.Count -ne 2) { throw 'Expected exactly two checksum entries' }
  $names = @($runtimeName, $sourceName)
  for ($i = 0; $i -lt 2; $i++) {
    if ($lines[$i] -cnotmatch ('^([0-9a-f]{64})  ' + [regex]::Escape($names[$i]) + '$')) { throw 'Invalid checksum entry or order' }
    if ((Get-FileHash -LiteralPath "$Directory/$($names[$i])").Hash -ine $Matches[1]) { throw 'Packaged checksum mismatch' }
  }
  $Archive = Join-Path $Directory $runtimeName
}
$Destination = Join-Path $work 'extracted'
$null = New-Item -ItemType Directory $Destination
$null = Invoke-Bounded (Get-Process -Id $PID).Path @('-NoProfile', '-STA', '-File', $PSCommandPath, '-ExtractWorker', '-Archive', $Archive, '-Destination', $Destination) 'shell-extraction'
Assert-Payload $Archive $Destination
if (-not $Synthetic) {
  if (@(Get-ChildItem -LiteralPath $Destination -Recurse -File | Where-Object { $_.Name -match '\.(tar\.gz|zip)$|^SHA256SUMS$' }).Count) { throw 'Source/checksum archive must remain outside runtime ZIP' }
  foreach ($required in @('mods.exe', 'BUILD-AND-SOURCE.md', 'LICENSE', 'COPYRIGHT.md', 'usvfs/usvfs_x86.dll', 'usvfs/usvfs_x64.dll', 'usvfs/usvfs_proxy_x86.exe', 'usvfs/usvfs_proxy_x64.exe')) {
    if (-not (Test-Path -LiteralPath "$Destination/$required" -PathType Leaf)) { throw "Missing runtime file: $required" }
  }
  $access = Get-Content -Raw -LiteralPath "$Destination/BUILD-AND-SOURCE.md"
  $sourceHash = (Get-FileHash -LiteralPath "$Directory/$sourceName").Hash.ToLowerInvariant()
  if (-not $access.Contains($sourceName) -or -not $access.Contains($sourceHash) -or $access -notmatch 'Source revision: [0-9a-f]{40}') { throw 'Source access details missing' }
  if ($ReleaseId.StartsWith('v')) {
    if (-not $access.Contains("https://github.com/Reilley64/mods/releases/download/$ReleaseId/$sourceName")) { throw 'Stable source URL missing' }
  } elseif (-not $access.Contains('same retained Actions artifact')) { throw 'Preview source access missing' }
  $help = Invoke-Bounded "$Destination/mods.exe" @('--help') 'help'
  if ($help -notmatch 'Usage:') { throw 'Packaged help output missing' }
  $version = Invoke-Bounded "$Destination/mods.exe" @('--version') 'version'
  if ($version.Trim() -cne "mods $ExpectedVersion") { throw "Unexpected version: $version" }
}
Remove-Item -LiteralPath $work -Recurse -Force
Write-Host 'Windows Shell payload smoke passed'
