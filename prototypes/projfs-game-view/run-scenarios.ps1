[CmdletBinding()]
param(
    [string]$Binary = "",
    [string]$Target = "",
    [string]$FixtureRoot = (Join-Path $env:TEMP "projfs-game-view-scenario-$PID"),
    [switch]$KeepFixture
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Assert-True([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw "ASSERTION FAILED: $Message" }
    Write-Host "PASS: $Message"
}

function Assert-Text([string]$Path, [string]$Expected, [string]$Message) {
    Assert-True (Test-Path -LiteralPath $Path -PathType Leaf) "$Message exists"
    $actual = Get-Content -LiteralPath $Path -Raw
    if ($actual -cne $Expected) {
        throw "ASSERTION FAILED: $Message. Expected '$Expected', got '$actual'"
    }
    Write-Host "PASS: $Message = '$actual'"
}

function Wait-Until([scriptblock]$Condition, [string]$Message, [int]$Seconds = 15) {
    $deadline = [DateTime]::UtcNow.AddSeconds($Seconds)
    do {
        if (& $Condition) { Write-Host "PASS: $Message"; return }
        Start-Sleep -Milliseconds 100
    } while ([DateTime]::UtcNow -lt $deadline)
    throw "TIMEOUT: $Message"
}

function Save-ProviderLogs($Process) {
    if ($null -eq $Process) { return }
    $stdout = $Process.StdoutTask.GetAwaiter().GetResult()
    $stderr = $Process.StderrTask.GetAwaiter().GetResult()
    [System.IO.File]::WriteAllText("$($Process.LogStem).stdout.log", $stdout)
    [System.IO.File]::WriteAllText("$($Process.LogStem).stderr.log", $stderr)
}

function Start-Provider([string]$Ready, [string]$Stop, [string]$LogStem) {
    Remove-Item -LiteralPath $Ready, $Stop -Force -ErrorAction SilentlyContinue
    $arguments = @(
        "serve", "--base", $base, "--view", $view, "--overwrite", $overwrite,
        "--state", $state, "--mod", $low, "--mod", $high,
        "--ready-file", $Ready, "--stop-file", $Stop
    )
    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $Binary
    $startInfo.UseShellExecute = $false
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    foreach ($argument in $arguments) { $startInfo.ArgumentList.Add($argument) }

    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    $process | Add-Member -NotePropertyName LogStem -NotePropertyValue $LogStem
    $started = $false
    try {
        if (-not $process.Start()) { throw "failed to start provider" }
        $started = $true
        $stdoutTask = $process.StandardOutput.ReadToEndAsync()
        $stderrTask = $process.StandardError.ReadToEndAsync()
        $process | Add-Member -NotePropertyName StdoutTask -NotePropertyValue $stdoutTask
        $process | Add-Member -NotePropertyName StderrTask -NotePropertyValue $stderrTask
        $deadline = [DateTime]::UtcNow.AddSeconds(20)
        while (-not (Test-Path -LiteralPath $Ready)) {
            if ($process.HasExited) {
                $process.WaitForExit()
                Save-ProviderLogs $process
                $errorText = $stderrTask.GetAwaiter().GetResult()
                throw "provider exited before ready (code $($process.ExitCode)): $errorText"
            }
            if ([DateTime]::UtcNow -ge $deadline) {
                $process.Kill($true)
                $process.WaitForExit()
                Save-ProviderLogs $process
                throw "provider did not become ready; see $LogStem.stderr.log"
            }
            Start-Sleep -Milliseconds 100
        }
        Write-Host "Provider ready, PID $($process.Id)"
        return $process
    } catch {
        if ($started) {
            if (-not $process.HasExited) { $process.Kill($true); $process.WaitForExit() }
            Save-ProviderLogs $process
        } else {
            [System.IO.File]::WriteAllText("$LogStem.stdout.log", "")
            [System.IO.File]::WriteAllText("$LogStem.stderr.log", "process start failed: $_")
        }
        $process.Dispose()
        throw
    }
}

function Stop-Provider($Process, [string]$Stop) {
    New-Item -ItemType File -Path $Stop -Force | Out-Null
    if (-not $Process.WaitForExit(20000)) {
        $Process.Kill($true)
        $Process.WaitForExit()
        Save-ProviderLogs $Process
        $Process.Dispose()
        throw "provider did not stop cleanly"
    }
    $Process.WaitForExit()
    Save-ProviderLogs $Process
    $exitCode = $Process.ExitCode
    $Process.Dispose()
    Assert-True ($exitCode -eq 0) "provider stopped with exit code 0"
}

function Cleanup-Provider($Process, [string]$Stop) {
    if ($null -eq $Process) { return }
    try {
        try { $hasExited = $Process.HasExited } catch { return }
        if (-not $hasExited) {
            New-Item -ItemType File -Path $Stop -Force -ErrorAction SilentlyContinue | Out-Null
            if (-not $Process.WaitForExit(3000)) {
                $Process.Kill($true)
            }
            $Process.WaitForExit()
        }
        Save-ProviderLogs $Process
    } finally {
        $Process.Dispose()
    }
}

function Assert-ProviderLogs([string]$LogStem) {
    Assert-True (Test-Path -LiteralPath "$LogStem.stdout.log" -PathType Leaf) "provider stdout log saved: $LogStem"
    Assert-True (Test-Path -LiteralPath "$LogStem.stderr.log" -PathType Leaf) "provider stderr log saved: $LogStem"
}

function Assert-SmallEnumeration {
    $actual = @(Get-ChildItem -LiteralPath (Join-Path $view "Data\nested") -Name | Sort-Object)
    $expected = @("base.txt", "high.txt", "low.txt")
    Assert-True (($actual -join ",") -ceq ($expected -join ",")) "nested merged enumeration is complete ($($actual -join ', '))"
}

function Assert-BulkEnumeration {
    $actual = @(Get-ChildItem -LiteralPath (Join-Path $view "Data\bulk") -Name | Sort-Object)
    $expected = @($bulkNames.ToArray()) + @("ENUM-DUPLICATE.TXT", "enum-collision")
    $expected = @($expected | Sort-Object)
    Assert-True (($actual -join "`n") -ceq ($expected -join "`n")) "large merged enumeration exactly matches $($expected.Count) winners"
}

function Invoke-Probe {
    Push-Location $view
    try {
        & ".\GameProbe.exe" probe `
            --expect "Data\winner.txt=overwrite" `
            --expect "Data\priority-high.txt=high-priority" `
            --expect "Data\priority-low.txt=low-priority" `
            --expect "Data\nested\base.txt=base-nested" `
            --expect "Data\nested\low.txt=low-nested" `
            --expect "Data\nested\high.txt=high-nested" `
            --expect "Data\new.txt=new-value" `
            --expect "Data\base-only.txt=changed-value" `
            --expect "Data\resurrect.txt=resurrected"
        Assert-True ($LASTEXITCODE -eq 0) "projected GameProbe.exe hydrated, launched, and read projected Data"
    } finally {
        Pop-Location
    }
}

function Invoke-NativeTool {
    Push-Location $view
    try {
        $output = @(& ".\NativeWhere.exe" "cmd.exe" 2>&1)
        Assert-True ($LASTEXITCODE -eq 0) "projected native where.exe copy launched"
        Assert-True (($output.Count -gt 0) -and (($output -join "`n") -match "cmd\.exe")) "projected native tool returned observable output"
        Write-Host "Native tool output: $($output -join '; ')"
    } finally {
        Pop-Location
    }
}

function Get-SourceSnapshot {
    $lines = [System.Collections.Generic.List[string]]::new()
    $roots = [ordered]@{ base = $base; low = $low; high = $high }
    foreach ($item in $roots.GetEnumerator()) {
        $label = $item.Key
        $rootPath = $item.Value
        foreach ($file in Get-ChildItem -LiteralPath $rootPath -File -Recurse | Sort-Object FullName) {
            $relative = [System.IO.Path]::GetRelativePath($rootPath, $file.FullName)
            $hash = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash
            $lines.Add("$label`t$relative`t$hash")
        }
    }
    return $lines.ToArray()
}

$provider1 = $null
$provider2 = $null
$provider3 = $null
$scenarioPassed = $false
try {
    if ([string]::IsNullOrWhiteSpace($Binary)) {
        Write-Host "Building release binary for the current Windows host..."
        $buildArguments = @("build", "--release", "--manifest-path", (Join-Path $PSScriptRoot "Cargo.toml"))
        if (-not [string]::IsNullOrWhiteSpace($Target)) { $buildArguments += @("--target", $Target) }
        & cargo @buildArguments
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed with exit code $LASTEXITCODE" }
        $metadataJson = & cargo metadata --format-version 1 --no-deps --manifest-path (Join-Path $PSScriptRoot "Cargo.toml")
        if ($LASTEXITCODE -ne 0) { throw "cargo metadata failed with exit code $LASTEXITCODE" }
        $targetDirectory = ($metadataJson | ConvertFrom-Json).target_directory
        if ([string]::IsNullOrWhiteSpace($Target)) {
            $Binary = Join-Path $targetDirectory "release\projfs-game-view.exe"
        } else {
            $Binary = Join-Path $targetDirectory "$Target\release\projfs-game-view.exe"
        }
    }
    $Binary = (Resolve-Path -LiteralPath $Binary).Path

    Remove-Item -LiteralPath $FixtureRoot -Recurse -Force -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Path $FixtureRoot | Out-Null

    $os = Get-CimInstance Win32_OperatingSystem
    $architecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
    $volume = Get-Volume -FilePath $FixtureRoot
    $feature = Get-WindowsOptionalFeature -Online -FeatureName Client-ProjFS
    Write-Host "OS: $($os.Caption), version $($os.Version), build $($os.BuildNumber)"
    Write-Host "Architecture: $architecture"
    Write-Host "Volume: $($volume.DriveLetter): filesystem=$($volume.FileSystem)"
    Write-Host "Client-ProjFS: $($feature.State)"
    Assert-True ([int]$os.BuildNumber -ge 22000) "Windows build is 22000 or newer"
    Assert-True ($architecture -in @("X64", "Arm64")) "architecture is X64 or Arm64"
    Assert-True ($volume.FileSystem -eq "NTFS") "fixture is on NTFS"
    Assert-True ($feature.State -eq "Enabled") "Client-ProjFS feature is enabled"

    $base = Join-Path $FixtureRoot "base"
    $view = Join-Path $FixtureRoot "view"
    $low = Join-Path $FixtureRoot "mod-low"
    $high = Join-Path $FixtureRoot "mod-high"
    $overwrite = Join-Path $FixtureRoot "overwrite"
    $stateDir = Join-Path $FixtureRoot "state"
    $state = Join-Path $stateDir "tombstones.txt"
    @($base, $low, $high, $overwrite, $stateDir) | ForEach-Object {
        New-Item -ItemType Directory -Path $_ -Force | Out-Null
    }
    @("Data\nested", "Data\delete-dir", "Data\collision", "Data\bulk") | ForEach-Object {
        New-Item -ItemType Directory -Path (Join-Path $base $_) -Force | Out-Null
    }
    @($low, $high) | ForEach-Object {
        New-Item -ItemType Directory -Path (Join-Path $_ "nested") -Force | Out-Null
        New-Item -ItemType Directory -Path (Join-Path $_ "bulk") -Force | Out-Null
    }
    New-Item -ItemType Directory -Path (Join-Path $overwrite "delete-dir") -Force | Out-Null

    Set-Content -LiteralPath (Join-Path $base "root.txt") -NoNewline -Value "base-root"
    Set-Content -LiteralPath (Join-Path $base "root-collision.txt") -NoNewline -Value "base-root-collision"
    Set-Content -LiteralPath (Join-Path $base "Data\winner.txt") -NoNewline -Value "base"
    Set-Content -LiteralPath (Join-Path $base "Data\priority-high.txt") -NoNewline -Value "base-priority"
    Set-Content -LiteralPath (Join-Path $base "Data\priority-low.txt") -NoNewline -Value "base-priority"
    Set-Content -LiteralPath (Join-Path $base "Data\base-only.txt") -NoNewline -Value "base-original"
    Set-Content -LiteralPath (Join-Path $base "Data\nested\base.txt") -NoNewline -Value "base-nested"
    Set-Content -LiteralPath (Join-Path $base "Data\delete-file.txt") -NoNewline -Value "base-delete"
    Set-Content -LiteralPath (Join-Path $base "Data\resurrect.txt") -NoNewline -Value "base-resurrect"
    Set-Content -LiteralPath (Join-Path $base "Data\delete-dir\child.txt") -NoNewline -Value "base-child"
    Set-Content -LiteralPath (Join-Path $base "Data\collision\hidden.txt") -NoNewline -Value "must-hide"

    Set-Content -LiteralPath (Join-Path $low "winner.txt") -NoNewline -Value "low"
    Set-Content -LiteralPath (Join-Path $low "priority-high.txt") -NoNewline -Value "low-priority"
    Set-Content -LiteralPath (Join-Path $low "priority-low.txt") -NoNewline -Value "low-priority"
    Set-Content -LiteralPath (Join-Path $low "low-only.txt") -NoNewline -Value "low-only"
    Set-Content -LiteralPath (Join-Path $low "nested\low.txt") -NoNewline -Value "low-nested"
    Set-Content -LiteralPath (Join-Path $low "root-collision.txt") -NoNewline -Value "low-data-root-collision"
    Set-Content -LiteralPath (Join-Path $low "mod-only-root.txt") -NoNewline -Value "only-under-data"

    Set-Content -LiteralPath (Join-Path $high "WINNER.TXT") -NoNewline -Value "high"
    Set-Content -LiteralPath (Join-Path $high "PRIORITY-HIGH.TXT") -NoNewline -Value "high-priority"
    Set-Content -LiteralPath (Join-Path $high "high-only.txt") -NoNewline -Value "high-only"
    Set-Content -LiteralPath (Join-Path $high "nested\high.txt") -NoNewline -Value "high-nested"
    Set-Content -LiteralPath (Join-Path $high "collision") -NoNewline -Value "file-wins"
    Set-Content -LiteralPath (Join-Path $high "root-collision.txt") -NoNewline -Value "high-data-root-collision"

    Set-Content -LiteralPath (Join-Path $overwrite "winner.txt") -NoNewline -Value "overwrite"
    Set-Content -LiteralPath (Join-Path $overwrite "delete-file.txt") -NoNewline -Value "overwrite-delete"
    Set-Content -LiteralPath (Join-Path $overwrite "delete-dir\own.txt") -NoNewline -Value "overwrite-child"
    Set-Content -LiteralPath (Join-Path $overwrite "root-collision.txt") -NoNewline -Value "overwrite-data-root-collision"
    Set-Content -LiteralPath (Join-Path $overwrite "overwrite-only-root.txt") -NoNewline -Value "only-under-data"

    $bulkNames = [System.Collections.Generic.List[string]]::new()
    for ($index = 0; $index -lt 480; $index++) {
        $name = "bulk-{0:D4}-{1}.txt" -f $index, ("x" * 120)
        $bulkNames.Add($name)
        $layerRoot = @((Join-Path $base "Data\bulk"), (Join-Path $low "bulk"), (Join-Path $high "bulk"))[$index % 3]
        Set-Content -LiteralPath (Join-Path $layerRoot $name) -NoNewline -Value "bulk-$index"
    }
    Set-Content -LiteralPath (Join-Path $base "Data\bulk\enum-duplicate.txt") -NoNewline -Value "base-duplicate"
    Set-Content -LiteralPath (Join-Path $low "bulk\Enum-Duplicate.txt") -NoNewline -Value "low-duplicate"
    Set-Content -LiteralPath (Join-Path $high "bulk\ENUM-DUPLICATE.TXT") -NoNewline -Value "high-duplicate"
    New-Item -ItemType Directory -Path (Join-Path $base "Data\bulk\enum-collision") | Out-Null
    Set-Content -LiteralPath (Join-Path $base "Data\bulk\enum-collision\hidden.txt") -NoNewline -Value "hidden"
    Set-Content -LiteralPath (Join-Path $low "bulk\enum-collision") -NoNewline -Value "file-wins"

    Copy-Item -LiteralPath $Binary -Destination (Join-Path $base "GameProbe.exe")
    $whereExe = Join-Path $env:SystemRoot "System32\where.exe"
    Assert-True (Test-Path -LiteralPath $whereExe -PathType Leaf) "native where.exe is available"
    Copy-Item -LiteralPath $whereExe -Destination (Join-Path $base "NativeWhere.exe")
    $sourceSnapshot = @(Get-SourceSnapshot)
    Write-Host "Source snapshot contains $($sourceSnapshot.Count) files"

    $ready1 = Join-Path $FixtureRoot "ready-1"
    $stop1 = Join-Path $FixtureRoot "stop-1"
    $provider1 = Start-Provider $ready1 $stop1 (Join-Path $FixtureRoot "provider-1")

    Assert-Text (Join-Path $view "root.txt") "base-root" "base root is projected"
    Assert-Text (Join-Path $view "root-collision.txt") "base-root-collision" "base root collision is retained"
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $view "mod-only-root.txt"))) "mod-only name is confined beneath Data"
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $view "overwrite-only-root.txt"))) "Overwrite-only name is confined beneath Data"
    Assert-Text (Join-Path $view "Data\WiNnEr.TxT") "overwrite" "case-insensitive Overwrite winner"
    $winnerEntries = @(Get-ChildItem -LiteralPath (Join-Path $view "Data") | Where-Object Name -ieq "winner.txt")
    Assert-True (($winnerEntries.Count -eq 1) -and ($winnerEntries[0].Name -ceq "winner.txt")) "directory enumeration preserves backing-store winner casing after a mixed-case first open"
    Assert-Text (Join-Path $view "Data\priority-high.txt") "high-priority" "high mod wins over low and base without Overwrite"
    Assert-Text (Join-Path $view "Data\priority-low.txt") "low-priority" "low mod wins over base without high or Overwrite"
    Assert-Text (Join-Path $view "Data\root-collision.txt") "overwrite-data-root-collision" "root-like layer names remain Data contributions"
    Assert-SmallEnumeration
    Assert-BulkEnumeration
    Assert-Text (Join-Path $view "Data\bulk\enum-duplicate.txt") "high-duplicate" "large enumeration case-duplicate uses high winner"
    Assert-Text (Join-Path $view "Data\bulk\enum-collision") "file-wins" "large enumeration collision uses low file winner"
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $view "Data\bulk\enum-collision\hidden.txt"))) "large enumeration file winner hides base directory descendants"
    Assert-Text (Join-Path $view "Data\collision") "file-wins" "higher-priority file beats lower directory"
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $view "Data\collision\hidden.txt"))) "file/directory collision hides lower descendants"

    Set-Content -LiteralPath (Join-Path $view "Data\new.txt") -NoNewline -Value "new-value"
    Wait-Until { (Test-Path -LiteralPath (Join-Path $overwrite "new.txt")) -and ((Get-Content -LiteralPath (Join-Path $overwrite "new.txt") -Raw) -ceq "new-value") } "new file mirrored to Overwrite after handle close"
    Set-Content -LiteralPath (Join-Path $view "Data\base-only.txt") -NoNewline -Value "changed-value"
    Wait-Until { (Test-Path -LiteralPath (Join-Path $overwrite "base-only.txt")) -and ((Get-Content -LiteralPath (Join-Path $overwrite "base-only.txt") -Raw) -ceq "changed-value") } "changed projected file mirrored to Overwrite after handle close"

    Remove-Item -LiteralPath (Join-Path $view "Data\delete-file.txt") -Force
    Wait-Until { (-not (Test-Path -LiteralPath (Join-Path $overwrite "delete-file.txt"))) -and (Test-Path -LiteralPath $state) -and ((Get-Content -LiteralPath $state -Raw) -match "(?m)^F\tData\\delete-file\.txt$") } "file deletion removes Overwrite item and persists file tombstone"
    Remove-Item -LiteralPath (Join-Path $view "Data\delete-dir") -Recurse -Force
    Wait-Until { (-not (Test-Path -LiteralPath (Join-Path $overwrite "delete-dir"))) -and (Test-Path -LiteralPath $state) -and ((Get-Content -LiteralPath $state -Raw) -match "(?m)^D\tData\\delete-dir$") } "directory deletion removes Overwrite item and persists directory tombstone"
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $view "Data\delete-dir\child.txt"))) "directory tombstone hides descendants"

    Remove-Item -LiteralPath (Join-Path $view "Data\resurrect.txt") -Force
    Wait-Until { (Test-Path -LiteralPath $state) -and ((Get-Content -LiteralPath $state -Raw) -match "(?m)^F\tData\\resurrect\.txt$") } "deletion records tombstone before recreation"
    Set-Content -LiteralPath (Join-Path $view "Data\resurrect.txt") -NoNewline -Value "resurrected"
    Wait-Until { (Test-Path -LiteralPath (Join-Path $overwrite "resurrect.txt")) -and ((Get-Content -LiteralPath (Join-Path $overwrite "resurrect.txt") -Raw) -ceq "resurrected") -and ((Get-Content -LiteralPath $state -Raw) -notmatch "(?m)^F\tData\\resurrect\.txt$") } "creating the same path clears its tombstone and mirrors it"

    Invoke-Probe
    Invoke-NativeTool
    Stop-Provider $provider1 $stop1
    $provider1 = $null
    Assert-ProviderLogs (Join-Path $FixtureRoot "provider-1")
    $pageLines = @(Select-String -Path (Join-Path $FixtureRoot "provider-1.stderr.log") -Pattern "enum page .*path=Data\\bulk " )
    Assert-True ($pageLines.Count -ge 2) "provider log proves large enumeration continued across $($pageLines.Count) callbacks"

    Write-Host "Restarting against the retained marked view and local full files..."
    $ready2 = Join-Path $FixtureRoot "ready-2"
    $stop2 = Join-Path $FixtureRoot "stop-2"
    $provider2 = Start-Provider $ready2 $stop2 (Join-Path $FixtureRoot "provider-2")
    Assert-Text (Join-Path $view "Data\new.txt") "new-value" "new local full file survives retained-view restart"
    Assert-Text (Join-Path $view "Data\base-only.txt") "changed-value" "changed local full file survives retained-view restart"
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $view "Data\delete-file.txt"))) "file deletion survives retained-view restart"
    Assert-SmallEnumeration
    Invoke-Probe
    Stop-Provider $provider2 $stop2
    $provider2 = $null
    Assert-ProviderLogs (Join-Path $FixtureRoot "provider-2")

    Remove-Item -LiteralPath $view -Recurse -Force
    New-Item -ItemType Directory -Path $view | Out-Null
    Write-Host "Deleted and recreated disposable view cache. Starting cold reconstruction..."
    $ready3 = Join-Path $FixtureRoot "ready-3"
    $stop3 = Join-Path $FixtureRoot "stop-3"
    $provider3 = Start-Provider $ready3 $stop3 (Join-Path $FixtureRoot "provider-3")
    Assert-Text (Join-Path $view "root.txt") "base-root" "base root survives cold reconstruction"
    Assert-Text (Join-Path $view "Data\new.txt") "new-value" "new Overwrite file survives cold reconstruction"
    Assert-Text (Join-Path $view "Data\base-only.txt") "changed-value" "changed Overwrite file survives cold reconstruction"
    Assert-Text (Join-Path $view "Data\resurrect.txt") "resurrected" "recreated path survives without a tombstone"
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $view "Data\delete-file.txt"))) "file tombstone survives cold reconstruction"
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $view "Data\delete-dir"))) "directory tombstone survives cold reconstruction"
    Assert-SmallEnumeration
    Assert-BulkEnumeration
    Invoke-Probe
    Invoke-NativeTool
    Stop-Provider $provider3 $stop3
    $provider3 = $null
    Assert-ProviderLogs (Join-Path $FixtureRoot "provider-3")

    $finalSourceSnapshot = @(Get-SourceSnapshot)
    $sourceDifference = @(Compare-Object -ReferenceObject $sourceSnapshot -DifferenceObject $finalSourceSnapshot -CaseSensitive)
    Assert-True ($sourceDifference.Count -eq 0) "every base/mod file path and SHA-256 hash is unchanged after all runs"

    Write-Host "Tombstone state after all runs:"
    Get-Content -LiteralPath $state | ForEach-Object { Write-Host "  $_" }
    $scenarioPassed = $true
    Write-Host "SCENARIO PASS"
    Write-Host "Fixture: $FixtureRoot"
} finally {
    Cleanup-Provider $provider1 (Join-Path $FixtureRoot "stop-1")
    Cleanup-Provider $provider2 (Join-Path $FixtureRoot "stop-2")
    Cleanup-Provider $provider3 (Join-Path $FixtureRoot "stop-3")
    if (-not $KeepFixture -and $scenarioPassed -and (Test-Path -LiteralPath $FixtureRoot)) {
        Remove-Item -LiteralPath $FixtureRoot -Recurse -Force -ErrorAction SilentlyContinue
    } elseif (-not $scenarioPassed -and (Test-Path -LiteralPath $FixtureRoot)) {
        Write-Host "Scenario failed; retained diagnostics at $FixtureRoot"
    }
}
