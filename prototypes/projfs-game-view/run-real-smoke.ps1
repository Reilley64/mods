[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$Binary,
    [Parameter(Mandatory)]
    [ValidatePattern("^[0-9A-Fa-f]{64}$")]
    [string]$ExpectedBinarySha256,
    [Parameter(Mandatory)]
    [string]$GameInstall,
    [ValidateSet("nvse_loader.exe", "FalloutNV.exe", "FalloutNVLauncher.exe")]
    [string]$Program = "nvse_loader.exe",
    [Parameter(Mandatory)]
    [string]$ScratchRoot,
    [ValidateRange(5, 120)]
    [int]$ObservationSeconds = 20,
    [ValidateRange(5, 120)]
    [int]$ReadyTimeoutSeconds = 20
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$targetNames = @("FalloutNV", "nvse_loader", "FalloutNVLauncher")
$provider = $null
$launched = $null
$launchedStarted = $false
$observedTargets = @{}
$ownedProcessHandles = @{}
$launchedRootIdentity = $null
$launchStartedUtc = $null
$launchedRootPid = $null
$launchRootCreationUtc = $null
$providerStoppedCleanly = $false
$providerStopLineObserved = $false
$projectedStartConfirmed = $false
$projectedStartMethod = "not-started"
$ownedCleanupComplete = $false
$ownershipProofComplete = $false
$quietIntervalAchieved = $false
$gameInventoryUnchanged = $false
$finalMatchingTargetsClear = $false
$logs = $null
$completionFile = $null
$smokePassed = $false
$failureMessage = $null
$currentSessionId = $null

function Assert-True([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw "ASSERTION FAILED: $Message" }
    Write-Host "PASS: $Message"
}

function Test-PathWithin([string]$Candidate, [string]$Root) {
    $candidateFull = [System.IO.Path]::GetFullPath($Candidate).TrimEnd('\')
    $rootFull = [System.IO.Path]::GetFullPath($Root).TrimEnd('\')
    return $candidateFull.Equals($rootFull, [StringComparison]::OrdinalIgnoreCase) -or
        $candidateFull.StartsWith("$rootFull\", [StringComparison]::OrdinalIgnoreCase)
}

function Assert-NoReparseAncestors([string]$Path) {
    $current = Get-Item -LiteralPath $Path
    while ($null -ne $current) {
        if (($current.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw "ScratchRoot parent chain contains a reparse point: $($current.FullName)"
        }
        $parent = if ($current -is [System.IO.FileInfo]) { $current.Directory } else { $current.Parent }
        if ($null -eq $parent -or $parent.FullName -eq $current.FullName) { break }
        $current = $parent
    }
}

function Get-ProcessPathSafe($Process) {
    try { return $Process.Path } catch {
        try { return $Process.MainModule.FileName } catch { return $null }
    }
}

function Get-ProcessStartUtcSafe($Process) {
    try { return $Process.StartTime.ToUniversalTime() } catch { return $null }
}

function Save-AsyncProcessLogs($Process, [string]$Stem) {
    if ($null -eq $Process) { return }
    $stdout = ""
    $stderr = ""
    if ($null -ne $Process.PSObject.Properties["StdoutTask"]) {
        $stdout = $Process.StdoutTask.GetAwaiter().GetResult()
    }
    if ($null -ne $Process.PSObject.Properties["StderrTask"]) {
        $stderr = $Process.StderrTask.GetAwaiter().GetResult()
    }
    [System.IO.File]::WriteAllText("$Stem.stdout.log", $stdout)
    [System.IO.File]::WriteAllText("$Stem.stderr.log", $stderr)
}

function Start-Provider {
    Remove-Item -LiteralPath $readyFile, $stopFile -Force -ErrorAction SilentlyContinue
    $arguments = @(
        "serve", "--base", $GameInstall, "--view", $view, "--overwrite", $overwrite,
        "--state", $stateFile, "--ready-file", $readyFile, "--stop-file", $stopFile
    )
    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $Binary
    $startInfo.UseShellExecute = $false
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    foreach ($argument in $arguments) { $startInfo.ArgumentList.Add($argument) }

    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    $started = $false
    try {
        if (-not $process.Start()) { throw "failed to start provider" }
        $started = $true
        $script:provider = $process
        $null = $process.Handle
        $stdoutTask = $process.StandardOutput.ReadToEndAsync()
        $stderrTask = $process.StandardError.ReadToEndAsync()
        $process | Add-Member -NotePropertyName StdoutTask -NotePropertyValue $stdoutTask
        $process | Add-Member -NotePropertyName StderrTask -NotePropertyValue $stderrTask
        $deadline = [DateTime]::UtcNow.AddSeconds($ReadyTimeoutSeconds)
        while (-not (Test-Path -LiteralPath $readyFile)) {
            $process.Refresh()
            if ($process.HasExited) {
                throw "provider exited before ready with code $($process.ExitCode)"
            }
            if ([DateTime]::UtcNow -ge $deadline) {
                throw "provider ready timeout"
            }
            Start-Sleep -Milliseconds 100
        }
        $process.Refresh()
        if ($process.HasExited) { throw "provider exited as readiness was observed" }
        Write-Host "Provider ready, PID $($process.Id)"
        return $process
    } catch {
        $originalError = $_
        if (-not $started) {
            try { [System.IO.File]::WriteAllText("$providerLogStem.stdout.log", "") } catch {}
            try { [System.IO.File]::WriteAllText("$providerLogStem.stderr.log", "provider start failed: $originalError") } catch {}
            try { $process.Dispose() } catch {}
            $script:provider = $null
        } else {
            try { $process.Refresh() } catch {}
            try { if (-not $process.HasExited) { $process.Kill($true) } } catch {
                try { $_ | Out-String | Set-Content -LiteralPath (Join-Path $logs "provider-start-kill-error.txt") } catch {}
            }
            try { $process.WaitForExit(10000) | Out-Null } catch {
                try { $_ | Out-String | Set-Content -LiteralPath (Join-Path $logs "provider-start-wait-error.txt") } catch {}
            }
            $exited = $false
            try { $process.Refresh(); $exited = $process.HasExited } catch {}
            if ($exited) {
                try { Save-AsyncProcessLogs $process $providerLogStem } catch {
                    try { $_ | Out-String | Set-Content -LiteralPath (Join-Path $logs "provider-start-log-error.txt") } catch {}
                }
                try { $process.Dispose() } catch {}
                $script:provider = $null
            }
        }
        throw $originalError
    }
}

function Stop-ProviderCleanly($Process) {
    if ($null -eq $Process) { return $false }
    $Process.Refresh()
    if ($Process.HasExited) {
        try { $Process.WaitForExit() } catch {}
        try { Save-AsyncProcessLogs $Process $providerLogStem } catch {}
        try { $Process.Dispose() } catch {}
        return $false
    }

    New-Item -ItemType File -Path $stopFile -Force | Out-Null
    $clean = $Process.WaitForExit(20000)
    if (-not $clean) {
        try { $Process.Kill($true) } catch {}
        try { $Process.WaitForExit(10000) | Out-Null } catch {}
    }
    $exited = $false
    try { $Process.Refresh(); $exited = $Process.HasExited } catch {}
    if (-not $exited) {
        throw "provider did not exit after stop timeout and forced termination attempt"
    }
    $exitCode = $null
    $logsSaved = $false
    if ($exited) {
        try { $Process.WaitForExit() } catch {}
        try {
            Save-AsyncProcessLogs $Process $providerLogStem
            $logsSaved = $true
            $providerStdoutLines = @(Get-Content -LiteralPath "$providerLogStem.stdout.log")
            $script:providerStopLineObserved = $providerStdoutLines -ccontains "provider stopped"
        } catch {
            try { $_ | Out-String | Set-Content -LiteralPath (Join-Path $logs "provider-stop-log-error.txt") } catch {}
        }
        try { $exitCode = $Process.ExitCode } catch {}
    }
    try { $Process.Dispose() } catch {}
    return $clean -and $exited -and $logsSaved -and $providerStopLineObserved -and ($exitCode -eq 0)
}

function Get-ProcessIdentity($Process) {
    $startUtc = Get-ProcessStartUtcSafe $Process
    if ($null -eq $startUtc) { return $null }
    return "$($Process.Id)|$($startUtc.ToString('o'))"
}

function Get-ParentProcessId([int]$ProcessId) {
    $cim = Get-CimInstance Win32_Process -Filter "ProcessId = $ProcessId" -ErrorAction SilentlyContinue
    if ($null -eq $cim) { return $null }
    return [int]$cim.ParentProcessId
}

function Test-RetainedProcessIdentity($Process, $Record) {
    try {
        $null = $Process.Handle
        $Process.Refresh()
        if ($Process.HasExited) { return $false }
        $startUtc = Get-ProcessStartUtcSafe $Process
        $path = Get-ProcessPathSafe $Process
        return $Process.Id -eq $Record.pid -and
            $Process.ProcessName -ceq $Record.name -and
            $null -ne $startUtc -and $startUtc.ToString("o") -ceq $Record.startUtc -and
            $null -ne $path -and $path.Equals($Record.executablePath, [StringComparison]::OrdinalIgnoreCase) -and
            $Process.SessionId -eq $Record.sessionId
    } catch { return $false }
}

function Test-LaunchedRootRecord($Record) {
    return $null -ne $launchedRootIdentity -and $Record.identity -ceq $launchedRootIdentity
}

function Capture-TargetProcesses {
    $candidates = [System.Collections.Generic.List[object]]::new()
    foreach ($process in Get-Process -Name $targetNames -ErrorAction SilentlyContinue) {
        $retained = $false
        try {
            $null = $process.Handle
            $process.Refresh()
            if ($process.HasExited) { continue }
            $identityKey = Get-ProcessIdentity $process
            if ($null -eq $identityKey) { continue }
            $path = Get-ProcessPathSafe $process
            $parentPid = Get-ParentProcessId $process.Id
            if (-not $observedTargets.ContainsKey($identityKey)) {
                $observedTargets[$identityKey] = [ordered]@{
                    identity = $identityKey
                    pid = $process.Id
                    parentPid = $parentPid
                    name = $process.ProcessName
                    executablePath = $path
                    startUtc = (Get-ProcessStartUtcSafe $process).ToString("o")
                    sessionId = $process.SessionId
                    firstObservedUtc = [DateTime]::UtcNow.ToString("o")
                    lastObservedUtc = [DateTime]::UtcNow.ToString("o")
                    owned = $false
                    ownershipReason = $null
                }
            } else {
                $observedTargets[$identityKey].lastObservedUtc = [DateTime]::UtcNow.ToString("o")
                $observedTargets[$identityKey].parentPid = $parentPid
                if ($null -eq $observedTargets[$identityKey].executablePath -and $null -ne $path) {
                    $observedTargets[$identityKey].executablePath = $path
                }
            }
            $record = $observedTargets[$identityKey]
            if (Test-LaunchedRootRecord $record) {
                $record.owned = $true
                $record.ownershipReason = "exact-launched-root-process-object"
                continue
            }
            if ($ownedProcessHandles.ContainsKey($identityKey)) { continue }
            $candidates.Add([pscustomobject]@{ process = $process; record = $record })
            $retained = $true
        } finally {
            if (-not $retained) { $process.Dispose() }
        }
    }

    $changed = $true
    while ($changed) {
        $changed = $false
        foreach ($candidate in @($candidates)) {
            if ($candidate.record.owned) { continue }
            if ($null -eq $candidate.record.executablePath -or
                $candidate.record.sessionId -ne $currentSessionId -or
                -not (Test-RetainedProcessIdentity $candidate.process $candidate.record)) {
                continue
            }
            $provedParentIdentity = $null
            foreach ($parentIdentity in @($ownedProcessHandles.Keys)) {
                $parentRecord = $observedTargets[$parentIdentity]
                if ($parentRecord.pid -ne $candidate.record.parentPid) { continue }
                $parentProcess = $ownedProcessHandles[$parentIdentity]
                if (Test-RetainedProcessIdentity $parentProcess $parentRecord) {
                    $provedParentIdentity = $parentIdentity
                    break
                }
            }
            if ($null -ne $provedParentIdentity) {
                $candidate.record.owned = $true
                $candidate.record.ownershipReason = "retained-handle-child-of:$provedParentIdentity"
                $ownedProcessHandles[$candidate.record.identity] = $candidate.process
                $changed = $true
            }
        }
    }
    foreach ($candidate in @($candidates)) {
        if (-not $candidate.record.owned) { $candidate.process.Dispose() }
    }
}

function Stop-OwnedTargetProcesses {
    Capture-TargetProcesses
    $cleanup = [System.Collections.Generic.List[object]]::new()
    foreach ($identityKey in @($ownedProcessHandles.Keys)) {
        $record = $observedTargets[$identityKey]
        $process = $ownedProcessHandles[$identityKey]
        $action = "already-exited"
        if (Test-LaunchedRootRecord $record) {
            $action = "launched-root-handled-by-original-process-object"
        } elseif (Test-RetainedProcessIdentity $process $record) {
            try {
                $process.Kill()
                if (-not $process.WaitForExit(10000)) { throw "owned target did not exit after Kill" }
                $action = "terminated-through-retained-exact-process-handle"
            } catch {
                $process.Refresh()
                if (-not $process.HasExited) { throw }
                $action = "exited-during-cleanup"
            }
        } elseif (-not $process.HasExited) {
            $action = "not-terminated-retained-identity-mismatch"
        }
        $cleanup.Add([pscustomobject]@{
            identity = $record.identity
            pid = $record.pid
            parentPid = $record.parentPid
            name = $record.name
            sessionId = $record.sessionId
            owned = $record.owned
            ownershipReason = $record.ownershipReason
            action = $action
        })
    }
    foreach ($record in @($observedTargets.Values | Where-Object { -not $_.owned })) {
        $cleanup.Add([pscustomobject]@{
            identity = $record.identity
            pid = $record.pid
            parentPid = $record.parentPid
            name = $record.name
            sessionId = $record.sessionId
            owned = $false
            ownershipReason = $null
            action = "observed-inconclusive-not-terminated"
        })
    }
    [ordered]@{ currentPid = $PID; currentSessionId = $currentSessionId; processes = @($cleanup) } |
        ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $logs "target-cleanup.json")
}

function Get-RemainingOwnedTargetProcesses {
    $remaining = [System.Collections.Generic.List[object]]::new()
    foreach ($identityKey in @($ownedProcessHandles.Keys)) {
        $record = $observedTargets[$identityKey]
        if (Test-LaunchedRootRecord $record) { continue }
        $process = $ownedProcessHandles[$identityKey]
        if (Test-RetainedProcessIdentity $process $record) { $remaining.Add($record) }
    }
    return @($remaining)
}

function Dispose-OwnedProcessHandles {
    foreach ($identityKey in @($ownedProcessHandles.Keys)) {
        if ($identityKey -ceq $launchedRootIdentity) { continue }
        try { $ownedProcessHandles[$identityKey].Dispose() } catch {}
    }
    $ownedProcessHandles.Clear()
}

function Stop-LaunchedRootProcess($Process) {
    $Process.Refresh()
    if (-not $Process.HasExited) {
        try { $Process.Kill() } catch {
            $Process.Refresh()
            if (-not $Process.HasExited) { throw }
        }
    }
    if (-not $Process.WaitForExit(10000)) { throw "launched root did not exit after cleanup" }
}

function Get-ExecutableSnapshot([string[]]$Paths) {
    $items = foreach ($path in $Paths) {
        if (Test-Path -LiteralPath $path -PathType Leaf) {
            $file = Get-Item -LiteralPath $path
            [pscustomobject]@{
                path = $file.FullName
                length = $file.Length
                lastWriteTimeUtc = $file.LastWriteTimeUtc.ToString("o")
                sha256 = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash
            }
        }
    }
    return @($items)
}

function Invoke-BoundedQuietCleanup([int]$QuietSeconds = 3, [int]$MaximumSeconds = 15) {
    $deadline = [DateTime]::UtcNow.AddSeconds($MaximumSeconds)
    $quietSince = [DateTime]::UtcNow
    $passes = [System.Collections.Generic.List[object]]::new()
    do {
        $beforeObservedCount = $observedTargets.Count
        Capture-TargetProcesses
        Stop-OwnedTargetProcesses
        $liveTargets = @(Get-Process -Name $targetNames -ErrorAction SilentlyContinue)
        $passes.Add([pscustomobject]@{
            capturedUtc = [DateTime]::UtcNow.ToString("o")
            currentSessionId = $currentSessionId
            liveTargets = @($liveTargets | ForEach-Object {
                [pscustomobject]@{ pid = $_.Id; sessionId = $_.SessionId; name = $_.ProcessName; startUtc = Get-ProcessStartUtcSafe $_; executablePath = Get-ProcessPathSafe $_ }
            })
            observedIdentityCount = $observedTargets.Count
        })
        foreach ($item in $liveTargets) { $item.Dispose() }
        if ($liveTargets.Count -gt 0 -or $observedTargets.Count -gt $beforeObservedCount) {
            $quietSince = [DateTime]::UtcNow
        } elseif (([DateTime]::UtcNow - $quietSince).TotalSeconds -ge $QuietSeconds) {
            ConvertTo-Json -InputObject (@($passes)) -Depth 5 | Set-Content -LiteralPath (Join-Path $logs "quiet-cleanup-passes.json")
            return $true
        }
        Start-Sleep -Milliseconds 250
    } while ([DateTime]::UtcNow -lt $deadline)
    ConvertTo-Json -InputObject (@($passes)) -Depth 5 | Set-Content -LiteralPath (Join-Path $logs "quiet-cleanup-passes.json")
    return $false
}

function Get-GameInstallInventory {
    $items = foreach ($file in Get-ChildItem -LiteralPath $GameInstall -File -Recurse -Force | Sort-Object FullName) {
        [pscustomobject]@{
            relativePath = [System.IO.Path]::GetRelativePath($GameInstall, $file.FullName)
            length = $file.Length
            lastWriteTimeUtc = $file.LastWriteTimeUtc.ToString("o")
        }
    }
    return @($items)
}

try {
    if ([System.IO.Path]::GetFileName($Program) -cne $Program -or [System.IO.Path]::IsPathRooted($Program)) {
        throw "-Program must be one root-level file name, not a path"
    }
    Assert-True (Test-Path -LiteralPath $Binary -PathType Leaf) "provider binary exists"
    $binaryLeaf = [System.IO.Path]::GetFileName($Binary)
    $forbiddenBinaryNames = @("steam.exe", "nvse_loader.exe", "FalloutNV.exe", "FalloutNVLauncher.exe")
    if ($binaryLeaf -in $forbiddenBinaryNames) { throw "-Binary must never name Steam or a game target executable" }
    Assert-True ($binaryLeaf -ceq "projfs-game-view.exe") "provider binary leaf name is exactly projfs-game-view.exe"
    $actualBinarySha256 = ((Get-FileHash -LiteralPath $Binary -Algorithm SHA256).Hash).ToUpperInvariant()
    Assert-True ($actualBinarySha256 -ceq $ExpectedBinarySha256.ToUpperInvariant()) "provider binary SHA-256 matches -ExpectedBinarySha256"
    Assert-NoReparseAncestors $Binary
    Assert-True (Test-Path -LiteralPath $GameInstall -PathType Container) "Game Installation exists"
    Assert-NoReparseAncestors $GameInstall
    $Binary = (Resolve-Path -LiteralPath $Binary).Path
    $GameInstall = (Resolve-Path -LiteralPath $GameInstall).Path
    $sourceProgram = Join-Path $GameInstall $Program
    Assert-True (Test-Path -LiteralPath $sourceProgram -PathType Leaf) "selected program exists in Game Installation root"
    Assert-NoReparseAncestors $sourceProgram

    $scratchInputFull = [System.IO.Path]::GetFullPath($ScratchRoot)
    if (Test-Path -LiteralPath $scratchInputFull) {
        throw "-ScratchRoot must not already exist: $scratchInputFull"
    }
    $scratchLeaf = Split-Path -Leaf $scratchInputFull
    $scratchParent = Split-Path -Parent $scratchInputFull
    Assert-True (-not [string]::IsNullOrWhiteSpace($scratchLeaf)) "ScratchRoot names a new directory"
    Assert-True (Test-Path -LiteralPath $scratchParent -PathType Container) "ScratchRoot parent exists"
    Assert-NoReparseAncestors $scratchParent
    $scratchParent = (Resolve-Path -LiteralPath $scratchParent).Path
    $scratchFull = Join-Path $scratchParent $scratchLeaf
    if ((Test-PathWithin $scratchFull $GameInstall) -or (Test-PathWithin $GameInstall $scratchFull)) {
        throw "ScratchRoot and Game Installation must not contain one another"
    }
    New-Item -ItemType Directory -Path $scratchFull | Out-Null
    $ScratchRoot = (Resolve-Path -LiteralPath $scratchFull).Path
    $view = Join-Path $ScratchRoot "view"
    $overwrite = Join-Path $ScratchRoot "overwrite"
    $stateDirectory = Join-Path $ScratchRoot "state"
    $control = Join-Path $ScratchRoot "control"
    $logs = Join-Path $ScratchRoot "logs"
    @($overwrite, $stateDirectory, $control, $logs) | ForEach-Object {
        New-Item -ItemType Directory -Path $_ | Out-Null
    }
    $stateFile = Join-Path $stateDirectory "tombstones.txt"
    $readyFile = Join-Path $control "ready"
    $stopFile = Join-Path $control "stop"
    $providerLogStem = Join-Path $logs "provider"
    $programLogStem = Join-Path $logs "program"
    $completionFile = Join-Path $control "real-smoke-complete.json"
    [ordered]@{
        path = $Binary
        leafName = $binaryLeaf
        expectedSha256 = $ExpectedBinarySha256.ToUpperInvariant()
        actualSha256 = $actualBinarySha256
        verifiedUtc = [DateTime]::UtcNow.ToString("o")
    } | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $logs "provider-binary-verification.json")

    $os = Get-CimInstance Win32_OperatingSystem
    $architecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
    $volume = Get-Volume -FilePath $ScratchRoot
    $currentProcess = [System.Diagnostics.Process]::GetCurrentProcess()
    $currentSessionId = $currentProcess.SessionId
    $explorerProcesses = @(Get-Process -Name "explorer" -ErrorAction SilentlyContinue | ForEach-Object {
        $item = [pscustomobject]@{
            pid = $_.Id
            sessionId = $_.SessionId
            executablePath = Get-ProcessPathSafe $_
            startUtc = Get-ProcessStartUtcSafe $_
        }
        $_.Dispose()
        $item
    })
    $sameSessionExplorer = @($explorerProcesses | Where-Object { $_.sessionId -eq $currentSessionId })
    [ordered]@{ currentPid = $PID; currentSessionId = $currentSessionId; processes = @($explorerProcesses) } |
        ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $logs "explorer-processes.json")
    $identity = [System.Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [System.Security.Principal.WindowsPrincipal]::new($identity)
    $isElevated = $principal.IsInRole([System.Security.Principal.WindowsBuiltInRole]::Administrator)
    $steamProcesses = @(Get-Process -Name "steam" -ErrorAction SilentlyContinue | ForEach-Object {
        $item = [pscustomobject]@{
            pid = $_.Id
            sessionId = $_.SessionId
            executablePath = Get-ProcessPathSafe $_
            startUtc = Get-ProcessStartUtcSafe $_
        }
        $_.Dispose()
        $item
    })
    [ordered]@{ currentPid = $PID; currentSessionId = $currentSessionId; processes = @($steamProcesses) } |
        ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $logs "steam-processes.json")
    $sameSessionSteam = @($steamProcesses | Where-Object { $_.sessionId -eq $currentSessionId })
    $environment = [ordered]@{
        capturedUtc = [DateTime]::UtcNow.ToString("o")
        osCaption = $os.Caption
        osVersion = $os.Version
        osBuild = $os.BuildNumber
        architecture = $architecture
        currentPid = $currentProcess.Id
        currentSessionId = $currentSessionId
        elevated = $isElevated
        providerBinarySha256 = $actualBinarySha256
        explorerProcessCount = $explorerProcesses.Count
        sameSessionExplorerProcessCount = $sameSessionExplorer.Count
        steamProcessCount = $steamProcesses.Count
        sameSessionSteamProcessCount = $sameSessionSteam.Count
        scratchRoot = $ScratchRoot
        scratchFileSystem = $volume.FileSystem
        projFsRuntimeCheck = "provider PrjStartVirtualizing must succeed"
        gameInstall = $GameInstall
        program = $Program
        observationSeconds = $ObservationSeconds
    }
    $environment | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $logs "environment.json")
    Assert-True ($os.Caption -match "Windows 11") "host is Windows 11"
    Assert-True ([int]$os.BuildNumber -ge 22000) "Windows build is 22000 or newer"
    Assert-True ($architecture -eq "X64") "host architecture is X64"
    Assert-True ($volume.FileSystem -eq "NTFS") "ScratchRoot is on NTFS"
    Assert-True ($currentSessionId -ne 0) "smoke process is not in Session 0"
    Assert-True ($sameSessionExplorer.Count -gt 0) "at least one explorer.exe process shares the smoke process session"
    Assert-True (-not $isElevated) "smoke process has a normal, non-elevated token"
    if ($steamProcesses.Count -gt 0) {
        Assert-True ($sameSessionSteam.Count -gt 0) "at least one running Steam process shares the smoke process session"
    }

    $preexisting = @(Get-Process -Name $targetNames -ErrorAction SilentlyContinue | ForEach-Object {
        $item = [pscustomobject]@{
            pid = $_.Id
            name = $_.ProcessName
            sessionId = $_.SessionId
            executablePath = Get-ProcessPathSafe $_
            startUtc = (Get-ProcessStartUtcSafe $_)
        }
        $_.Dispose()
        $item
    })
    [ordered]@{ currentPid = $PID; currentSessionId = $currentSessionId; processes = @($preexisting) } |
        ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $logs "preexisting-target-processes.json")
    Assert-True ($preexisting.Count -eq 0) "no FalloutNV, nvse_loader, or FalloutNVLauncher process is already running"

    $selectedExecutables = @(
        $sourceProgram,
        (Join-Path $GameInstall "FalloutNV.exe"),
        (Join-Path $GameInstall "FalloutNVLauncher.exe")
    ) | Select-Object -Unique
    $beforeSnapshot = @(Get-ExecutableSnapshot $selectedExecutables)
    ConvertTo-Json -InputObject (@($beforeSnapshot)) -Depth 4 | Set-Content -LiteralPath (Join-Path $logs "source-executables-before.json")
    $beforeGameInventory = @(Get-GameInstallInventory)
    ConvertTo-Json -InputObject (@($beforeGameInventory)) -Depth 4 | Set-Content -LiteralPath (Join-Path $logs "game-install-inventory-before.json")
    Write-Host "Game Installation inventory contains $($beforeGameInventory.Count) files"

    $provider = Start-Provider
    Assert-True ($null -ne $provider -and -not $provider.HasExited) "normal-integrity ProjFS provider started successfully"
    Assert-True ($provider.SessionId -eq $currentSessionId) "provider runs in the smoke process session"
    [ordered]@{
        pid = $provider.Id
        sessionId = $provider.SessionId
        currentSessionId = $currentSessionId
        capturedUtc = [DateTime]::UtcNow.ToString("o")
    } | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $logs "provider-process.json")
    $rootListing = @(Get-ChildItem -LiteralPath $view -Name | Sort-Object)
    $dataListing = if (Test-Path -LiteralPath (Join-Path $view "Data") -PathType Container) {
        @(Get-ChildItem -LiteralPath (Join-Path $view "Data") -Name | Sort-Object)
    } else { @() }
    [ordered]@{
        capturedUtc = [DateTime]::UtcNow.ToString("o")
        rootEntries = $rootListing
        dataEntries = $dataListing
        projectedProgram = (Get-Item -LiteralPath (Join-Path $view $Program)).FullName
    } | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $logs "projected-listings-before-launch.json")
    Assert-True ($rootListing -contains $Program) "projected root enumeration contains selected program"
    $justBeforeLaunch = @(Get-Process -Name $targetNames -ErrorAction SilentlyContinue | ForEach-Object {
        $item = [pscustomobject]@{ pid = $_.Id; name = $_.ProcessName; sessionId = $_.SessionId; startUtc = Get-ProcessStartUtcSafe $_; executablePath = Get-ProcessPathSafe $_ }
        $_.Dispose()
        $item
    })
    [ordered]@{ currentPid = $PID; currentSessionId = $currentSessionId; processes = @($justBeforeLaunch) } |
        ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $logs "target-processes-immediately-before-launch.json")
    Assert-True ($justBeforeLaunch.Count -eq 0) "no matching target process appeared before launch"

    $projectedProgram = Join-Path $view $Program
    $launchInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $launchInfo.FileName = $projectedProgram
    $launchInfo.WorkingDirectory = $view
    $launchInfo.UseShellExecute = $false
    $launched = [System.Diagnostics.Process]::new()
    $launched.StartInfo = $launchInfo
    $launchStartedUtc = [DateTime]::UtcNow
    $launchStopwatch = [System.Diagnostics.Stopwatch]::StartNew()
    if (-not $launched.Start()) { throw "selected projected program did not start" }
    $launchedStarted = $true
    $null = $launched.Handle
    $launched.Refresh()
    $launchedRootPid = $launched.Id
    $launchRootCreationUtc = Get-ProcessStartUtcSafe $launched
    $launchedPath = try { $launched.MainModule.FileName } catch { $null }
    [ordered]@{
        requestedProjectedPath = $projectedProgram
        workingDirectory = $view
        currentSessionId = $currentSessionId
        rootPid = $launchedRootPid
        rootSessionId = $launched.SessionId
        rootStartUtc = if ($null -ne $launchRootCreationUtc) { $launchRootCreationUtc.ToString("o") } else { $null }
        observedExecutablePath = $launchedPath
        capturedUtc = [DateTime]::UtcNow.ToString("o")
    } | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $logs "launch-start-attempt.json")
    if ($null -eq $launchRootCreationUtc -or $null -eq $launchedPath) {
        throw "launched root identity/path could not be verified through its retained Process handle"
    }
    if ($launched.SessionId -ne $currentSessionId) {
        throw "launched root process is not in the smoke process session"
    }
    $projectedStartConfirmed = [System.IO.Path]::GetFullPath($launchedPath).Equals(
        [System.IO.Path]::GetFullPath($projectedProgram),
        [StringComparison]::OrdinalIgnoreCase
    )
    if (-not $projectedStartConfirmed) {
        throw "launched process image path does not match the projected executable path: $launchedPath"
    }
    $projectedStartMethod = "retained-root-handle-and-exact-MainModule-path"
    $launchedRootIdentity = Get-ProcessIdentity $launched
    $rootRecord = [ordered]@{
        identity = $launchedRootIdentity
        pid = $launchedRootPid
        parentPid = Get-ParentProcessId $launchedRootPid
        name = $launched.ProcessName
        executablePath = $launchedPath
        startUtc = $launchRootCreationUtc.ToString("o")
        sessionId = $launched.SessionId
        firstObservedUtc = [DateTime]::UtcNow.ToString("o")
        lastObservedUtc = [DateTime]::UtcNow.ToString("o")
        owned = $true
        ownershipReason = "exact-launched-root-process-object"
    }
    $observedTargets[$launchedRootIdentity] = $rootRecord
    $ownedProcessHandles[$launchedRootIdentity] = $launched
    [ordered]@{
        requestedProjectedPath = $projectedProgram
        workingDirectory = $view
        currentSessionId = $currentSessionId
        startUtc = $launchStartedUtc.ToString("o")
        rootPid = $launched.Id
        rootStartUtc = $launchRootCreationUtc.ToString("o")
        rootSessionId = $launched.SessionId
        initiallyObservedExecutablePath = $launchedPath
        initialConfirmationMethod = $projectedStartMethod
    } | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $logs "launch-start.json")

    $deadline = [DateTime]::UtcNow.AddSeconds($ObservationSeconds)
    do {
        Capture-TargetProcesses
        Start-Sleep -Milliseconds 250
    } while ([DateTime]::UtcNow -lt $deadline)
    Capture-TargetProcesses

    $rootRecord = @($observedTargets.Values | Where-Object {
        Test-LaunchedRootRecord $_
    } | Select-Object -First 1)
    if ($null -eq $launchedPath -and $rootRecord.Count -eq 1) {
        $laterPath = $rootRecord[0].executablePath
        if ($null -ne $laterPath) {
            $launchedPath = $laterPath
            $projectedStartConfirmed = [System.IO.Path]::GetFullPath($launchedPath).Equals(
                [System.IO.Path]::GetFullPath($projectedProgram),
                [StringComparison]::OrdinalIgnoreCase
            )
            $projectedStartMethod = if ($projectedStartConfirmed) { "observed-process-path-during-window" } else { "observed-path-mismatch" }
        }
    }
    $launched.Refresh()
    $rootExitedDuringObservation = $launched.HasExited
    $rootExitCode = if ($rootExitedDuringObservation) { $launched.ExitCode } else { $null }
    $rootExitUtc = if ($rootExitedDuringObservation) { [DateTime]::UtcNow.ToString("o") } else { $null }
    [ordered]@{
        requestedProjectedPath = $projectedProgram
        workingDirectory = $view
        currentSessionId = $currentSessionId
        startUtc = $launchStartedUtc.ToString("o")
        rootPid = $launched.Id
        observedExecutablePath = $launchedPath
        projectedStartConfirmed = $projectedStartConfirmed
        confirmationMethod = $projectedStartMethod
        observationSeconds = $ObservationSeconds
        observationEndedUtc = [DateTime]::UtcNow.ToString("o")
        observedElapsedMilliseconds = $launchStopwatch.ElapsedMilliseconds
        exitedDuringObservation = $rootExitedDuringObservation
        exitCode = $rootExitCode
        exitObservedUtc = $rootExitUtc
        matchingProcessesObserved = @($observedTargets.Values)
    } | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $logs "launch-evidence.json")

    $rootTerminatedBySmoke = -not $rootExitedDuringObservation
    Capture-TargetProcesses
    Stop-LaunchedRootProcess $launched
    Stop-OwnedTargetProcesses
    $quietIntervalAchieved = Invoke-BoundedQuietCleanup -QuietSeconds 3 -MaximumSeconds 15
    [ordered]@{
        pid = $launched.Id
        rootSessionId = $launched.SessionId
        currentSessionId = $currentSessionId
        finalExitCode = $launched.ExitCode
        terminatedBySmoke = $rootTerminatedBySmoke
        cleanupCompletedUtc = [DateTime]::UtcNow.ToString("o")
        elapsedMilliseconds = $launchStopwatch.ElapsedMilliseconds
    } | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $logs "program-final.json")
    $remainingOwnedTargets = @(Get-RemainingOwnedTargetProcesses)
    $ownedCleanupComplete = $remainingOwnedTargets.Count -eq 0
    Write-Host "Bounded quiet interval achieved: $quietIntervalAchieved"
    Write-Host "Owned matching targets remaining after cleanup: $($remainingOwnedTargets.Count)"
    $unprovenTargets = @($observedTargets.Values | Where-Object {
        -not $_.owned -and -not (Test-LaunchedRootRecord $_)
    })
    [ordered]@{ currentPid = $PID; currentSessionId = $currentSessionId; processes = @($unprovenTargets) } |
        ConvertTo-Json -Depth 7 | Set-Content -LiteralPath (Join-Path $logs "unproven-target-processes.json")
    $ownershipProofComplete = $unprovenTargets.Count -eq 0
    Write-Host "Matching targets without ownership proof: $($unprovenTargets.Count)"
    [System.IO.File]::WriteAllText("$programLogStem.stdout.log", "Program output inherited the smoke-test console and was not redirected, to avoid retaining pipes in spawned processes.`n")
    [System.IO.File]::WriteAllText("$programLogStem.stderr.log", "Program output inherited the smoke-test console and was not redirected, to avoid retaining pipes in spawned processes.`n")
    $launched.Dispose()
    $launched = $null

    $providerStoppedCleanly = Stop-ProviderCleanly $provider
    $provider = $null

    $overwriteOutput = @(Get-ChildItem -LiteralPath $overwrite -File -Recurse | ForEach-Object {
        [pscustomobject]@{
            relativePath = [System.IO.Path]::GetRelativePath($overwrite, $_.FullName)
            length = $_.Length
            lastWriteTimeUtc = $_.LastWriteTimeUtc.ToString("o")
        }
    })
    ConvertTo-Json -InputObject (@($overwriteOutput)) -Depth 4 | Set-Content -LiteralPath (Join-Path $logs "overwrite-files.json")
    $tombstoneText = if (Test-Path -LiteralPath $stateFile -PathType Leaf) { Get-Content -LiteralPath $stateFile -Raw } else { "" }
    [System.IO.File]::WriteAllText((Join-Path $logs "tombstones.txt"), $tombstoneText)
    Write-Host "Physical Overwrite files: $($overwriteOutput.Count)"
    foreach ($item in $overwriteOutput) { Write-Host "  $($item.relativePath) ($($item.length) bytes)" }
    Write-Host "Tombstone state:"
    if ([string]::IsNullOrEmpty($tombstoneText)) { Write-Host "  <none>" } else { Write-Host $tombstoneText }

    $afterSnapshot = @(Get-ExecutableSnapshot $selectedExecutables)
    ConvertTo-Json -InputObject (@($afterSnapshot)) -Depth 4 | Set-Content -LiteralPath (Join-Path $logs "source-executables-after.json")
    $beforeJson = ConvertTo-Json -InputObject (@($beforeSnapshot)) -Depth 4 -Compress
    $afterJson = ConvertTo-Json -InputObject (@($afterSnapshot)) -Depth 4 -Compress
    $sourceExecutablesUnchanged = $beforeJson -ceq $afterJson
    $afterGameInventory = @(Get-GameInstallInventory)
    ConvertTo-Json -InputObject (@($afterGameInventory)) -Depth 4 | Set-Content -LiteralPath (Join-Path $logs "game-install-inventory-after.json")
    $gameInventoryDifferences = @(Compare-Object -ReferenceObject $beforeGameInventory -DifferenceObject $afterGameInventory -Property relativePath, length, lastWriteTimeUtc -CaseSensitive)
    ConvertTo-Json -InputObject (@($gameInventoryDifferences)) -Depth 5 | Set-Content -LiteralPath (Join-Path $logs "game-install-inventory-differences.json")
    $gameInventoryUnchanged = $gameInventoryDifferences.Count -eq 0

    Capture-TargetProcesses
    $finalSystemTargets = @(Get-Process -Name $targetNames -ErrorAction SilentlyContinue | ForEach-Object {
        $item = [pscustomobject]@{
            pid = $_.Id
            name = $_.ProcessName
            sessionId = $_.SessionId
            startUtc = Get-ProcessStartUtcSafe $_
            executablePath = Get-ProcessPathSafe $_
        }
        $_.Dispose()
        $item
    })
    [ordered]@{ currentPid = $PID; currentSessionId = $currentSessionId; processes = @($finalSystemTargets) } |
        ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $logs "final-system-target-processes.json")
    $finalMatchingTargetsClear = $finalSystemTargets.Count -eq 0
    $unprovenTargets = @($observedTargets.Values | Where-Object { -not $_.owned })
    [ordered]@{ currentPid = $PID; currentSessionId = $currentSessionId; processes = @($unprovenTargets) } |
        ConvertTo-Json -Depth 7 | Set-Content -LiteralPath (Join-Path $logs "unproven-target-processes.json")
    $ownershipProofComplete = $unprovenTargets.Count -eq 0

    $spawnedFalloutObserved = @($observedTargets.Values | Where-Object { $_.name -eq "FalloutNV" -and $_.owned }).Count -gt 0
    [ordered]@{
        projectedStartConfirmed = $projectedStartConfirmed
        confirmationMethod = $projectedStartMethod
        spawnedFalloutObserved = $spawnedFalloutObserved
        matchingProcessesObserved = $observedTargets.Count
        ownedCleanupComplete = $ownedCleanupComplete
        ownershipProofComplete = $ownershipProofComplete
        quietIntervalAchieved = $quietIntervalAchieved
        finalMatchingTargetsClear = $finalMatchingTargetsClear
        providerStoppedCleanly = $providerStoppedCleanly
        providerStopLineObserved = $providerStopLineObserved
        sourceExecutablesUnchanged = $sourceExecutablesUnchanged
        gameInventoryUnchanged = $gameInventoryUnchanged
        realSmokePassConditionsMet = $projectedStartConfirmed -and $ownedCleanupComplete -and
            $ownershipProofComplete -and $quietIntervalAchieved -and $finalMatchingTargetsClear -and
            $providerStoppedCleanly -and $providerStopLineObserved -and
            $sourceExecutablesUnchanged -and $gameInventoryUnchanged
    } | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $logs "summary.json")
    Assert-True $sourceExecutablesUnchanged "selected Game Installation executable metadata and hashes are unchanged"
    Assert-True $projectedStartConfirmed "selected executable started through the projected view path"
    Assert-True $ownedCleanupComplete "all proven-owned matching Fallout/NVSE descendants stopped"
    Assert-True $ownershipProofComplete "no matching process lacked retained-handle ancestry proof"
    Assert-True $quietIntervalAchieved "bounded cleanup reached a three-second system-wide quiet interval"
    Assert-True $finalMatchingTargetsClear "final fresh system-wide target query is empty"
    Assert-True $providerStoppedCleanly "provider stopped cleanly"
    Assert-True $providerStopLineObserved "provider stdout contains the exact provider stopped line"
    Assert-True $gameInventoryUnchanged "Game Installation path/length/write-time inventory has no observable differences"

    Write-Host "Matching Fallout/NVSE processes observed: $($observedTargets.Count)"
    foreach ($record in $observedTargets.Values) {
        Write-Host "  PID $($record.pid) $($record.name) path=$($record.executablePath)"
    }
    Dispose-OwnedProcessHandles
    $launchedStarted = $false
    [ordered]@{
        status = "PASS"
        completedUtc = [DateTime]::UtcNow.ToString("o")
        currentPid = $PID
        currentSessionId = $currentSessionId
        failure = $null
        evidenceRoot = $ScratchRoot
    } | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $completionFile
    $smokePassed = $true
    Write-Host "REAL SMOKE PASS"
    Write-Host "Evidence preserved at $ScratchRoot"
} catch {
    $failureMessage = $_.Exception.ToString()
    throw
} finally {
    if ($null -ne $launched) {
        if ($launchedStarted) {
            try { Capture-TargetProcesses } catch {
                if ($null -ne $logs) { try { $_ | Out-String | Set-Content -LiteralPath (Join-Path $logs "target-capture-cleanup-error.txt") } catch {} }
            }
            try { Stop-LaunchedRootProcess $launched } catch {
                if ($null -ne $logs) { try { $_ | Out-String | Set-Content -LiteralPath (Join-Path $logs "program-cleanup-error.txt") } catch {} }
            }
            try { Stop-OwnedTargetProcesses } catch {
                if ($null -ne $logs) { try { $_ | Out-String | Set-Content -LiteralPath (Join-Path $logs "target-cleanup-error.txt") } catch {} }
            }
        }
        if ($null -ne $logs) {
            try { [System.IO.File]::WriteAllText((Join-Path $logs "program-cleanup.txt"), "Launched root cleanup attempted at $([DateTime]::UtcNow.ToString('o')).`n") } catch {}
            try { [System.IO.File]::WriteAllText((Join-Path $logs "program.stdout.log"), "Program output inherited the smoke-test console and was not redirected.`n") } catch {}
            try { [System.IO.File]::WriteAllText((Join-Path $logs "program.stderr.log"), "Program output inherited the smoke-test console and was not redirected.`n") } catch {}
        }
        try { $launched.Dispose() } catch {}
    } elseif ($launchedStarted -and $null -ne $launchStartedUtc) {
        try { Stop-OwnedTargetProcesses } catch {
            if ($null -ne $logs) { try { $_ | Out-String | Set-Content -LiteralPath (Join-Path $logs "target-cleanup-error.txt") } catch {} }
        }
    }
    try { Dispose-OwnedProcessHandles } catch {
        if ($null -ne $logs) { try { $_ | Out-String | Set-Content -LiteralPath (Join-Path $logs "owned-handle-dispose-error.txt") } catch {} }
    }
    if ($null -ne $provider) {
        try { $providerStoppedCleanly = Stop-ProviderCleanly $provider } catch {
            $providerCleanupError = $_
            try { $provider.Refresh() } catch {}
            try { if (-not $provider.HasExited) { $provider.Kill($true) } } catch {
                if ($null -ne $logs) { try { $_ | Out-String | Set-Content -LiteralPath (Join-Path $logs "provider-final-kill-error.txt") } catch {} }
            }
            try { $provider.WaitForExit(10000) | Out-Null } catch {
                if ($null -ne $logs) { try { $_ | Out-String | Set-Content -LiteralPath (Join-Path $logs "provider-final-wait-error.txt") } catch {} }
            }
            $providerExited = $false
            try { $provider.Refresh(); $providerExited = $provider.HasExited } catch {}
            if ($providerExited -and $null -ne $logs) {
                try { Save-AsyncProcessLogs $provider (Join-Path $logs "provider") } catch {
                    try { $_ | Out-String | Set-Content -LiteralPath (Join-Path $logs "provider-final-log-error.txt") } catch {}
                }
            }
            try { $provider.Dispose() } catch {}
            if ($null -ne $logs) { try { $providerCleanupError | Out-String | Set-Content -LiteralPath (Join-Path $logs "provider-cleanup-error.txt") } catch {} }
        }
    }
    if ($null -ne $completionFile) {
        try {
            if (-not $smokePassed) {
                [ordered]@{
                    status = "FAILED"
                    completedUtc = [DateTime]::UtcNow.ToString("o")
                    currentPid = $PID
                    currentSessionId = $currentSessionId
                    failure = $failureMessage
                    evidenceRoot = $ScratchRoot
                } | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $completionFile
            }
            Write-Host "REAL SMOKE COMPLETE: $completionFile"
        } catch { Write-Host "REAL SMOKE COMPLETION MARKER ERROR: $($_.Exception.Message)" }
    }
    if ($null -ne $ScratchRoot -and (Test-Path -LiteralPath $ScratchRoot)) {
        Write-Host "Smoke fixture preserved at $ScratchRoot"
    }
}
