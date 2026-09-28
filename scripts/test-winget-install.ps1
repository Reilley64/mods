# Run only on a disposable GitHub-hosted Windows runner. No archive fallback.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$ManifestDirectory,
    [Parameter(Mandatory)][ValidatePattern('^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$')][string]$Version
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted' -or $env:OS -ne 'Windows_NT') {
    throw 'This gate requires a disposable GitHub-hosted Windows runner.'
}

$evidence = Join-Path $env:GITHUB_WORKSPACE 'dist/winget-evidence'
New-Item -ItemType Directory -Force -Path $evidence | Out-Null
$packageId = 'Reilley64.Mods'
$aliasPath = Join-Path $env:LOCALAPPDATA 'Microsoft/WinGet/Links/mods.exe'
$machineAlias = Join-Path $env:ProgramFiles 'WinGet/Links/mods.exe'
$nativeLogs = Join-Path $env:LOCALAPPDATA 'Packages/Microsoft.DesktopAppInstaller_8wekyb3d8bbwe/LocalState/DiagOutputDir'
$noApplications = -1978335212 # APPINSTALLER_CLI_ERROR_NO_APPLICATIONS_FOUND (0x8A150014)
$restoreLocalManifests = $false
$installAttempted = $false
$originalPath = $env:PATH
$failures = [System.Collections.Generic.List[string]]::new()
$script:commandNumber = 0

function Invoke-Recorded {
    param([string]$Executable, [string[]]$Arguments, [int[]]$AllowedExitCodes = @(0))
    $script:commandNumber++
    $prefix = Join-Path $evidence ('{0:D2}' -f $script:commandNumber)
    @{ executable = $Executable; arguments = $Arguments } | ConvertTo-Json -Depth 4 | Set-Content "$prefix.command.json"
    & $Executable @Arguments 1> "$prefix.stdout.txt" 2> "$prefix.stderr.txt"
    $exitCode = $LASTEXITCODE
    Set-Content "$prefix.exit-code.txt" $exitCode
    if ($exitCode -notin $AllowedExitCodes) {
        throw "Native command failed ($exitCode): $Executable $($Arguments -join ' '); see $prefix.*"
    }
    [pscustomobject]@{ ExitCode = $exitCode; Stdout = (Get-Content "$prefix.stdout.txt" -Raw) }
}

try {
    $winget = (Get-Command winget.exe -CommandType Application -ErrorAction Stop).Source
    Invoke-Recorded $winget @('--info') | Out-Null
    $osVersion = [Environment]::OSVersion.Version
    Set-Content (Join-Path $evidence 'os-version.txt') $osVersion.ToString()
    if ($osVersion -lt [version]'10.0.22000.0') {
        throw 'Manifest requires Windows 10.0.22000.0 or later; windows-2022 (20348) is not compatible. Use a compatible runner; do not bypass this requirement.'
    }
    $manifest = (Resolve-Path -LiteralPath $ManifestDirectory).Path
    $yamlFiles = @(Get-ChildItem -LiteralPath $manifest -File -Filter '*.yaml')
    if ($yamlFiles.Count -eq 0) { throw 'No YAML manifests found.' }
    foreach ($yamlFile in $yamlFiles) {
        $yaml = Get-Content $yamlFile.FullName -Raw
        # Generated Mods manifests use plain scalar identity fields. Reject other input.
        if ($yaml -notmatch '(?m)^PackageIdentifier: Reilley64\.Mods\s*$' -or
            $yaml -notmatch ('(?m)^PackageVersion: ' + [regex]::Escape($Version) + '\s*$')) {
            throw "Unexpected manifest identity: $($yamlFile.Name)"
        }
    }
    Copy-Item -LiteralPath $manifest -Destination (Join-Path $evidence 'manifests') -Recurse
    Invoke-Recorded $winget @('validate', '--manifest', $manifest, '--disable-interactivity', '--verbose-logs') | Out-Null

    # Check all scopes before claiming ownership of an attempted install.
    $listArguments = @('list', '--id', $packageId, '--exact', '--accept-source-agreements', '--disable-interactivity', '--verbose-logs')
    $before = Invoke-Recorded $winget $listArguments @(0, $noApplications)
    if ($before.ExitCode -eq 0) { throw 'Mods is already installed; refusing to modify it.' }
    foreach ($link in @($aliasPath, $machineAlias)) {
        if (Get-Item -LiteralPath $link -Force -ErrorAction SilentlyContinue) { throw "Preexisting alias: $link" }
    }
    if (Get-Command mods -ErrorAction SilentlyContinue) { throw 'A mods command already exists on PATH.' }

    $settings = (Invoke-Recorded $winget @('settings', 'export', '--disable-interactivity')).Stdout | ConvertFrom-Json
    $localManifests = $settings.adminSettings.LocalManifestFiles
    if ($localManifests -isnot [bool]) { throw 'Cannot determine LocalManifestFiles state.' }
    if (Test-Path -LiteralPath $settings.userSettingsFile) {
        $settingsText = Get-Content -LiteralPath $settings.userSettingsFile -Raw
        Copy-Item -LiteralPath $settings.userSettingsFile -Destination (Join-Path $evidence 'user-settings-before.json')
        # Even a commented override is rejected rather than risk testing tar.
        if ($settingsText -match 'archiveExtractionMethod') {
            throw 'Explicit archive extraction override found; this gate requires the default ZIP handler.'
        }
    }
    if (-not $localManifests) {
        # Restore even if enable fails after changing state.
        $restoreLocalManifests = $true
        Invoke-Recorded $winget @('settings', '--enable', 'LocalManifestFiles', '--disable-interactivity', '--verbose-logs') | Out-Null
    }

    $installAttempted = $true
    Invoke-Recorded $winget @('install', '--manifest', $manifest, '--scope', 'user', '--silent', '--accept-package-agreements', '--accept-source-agreements', '--disable-interactivity', '--verbose-logs') | Out-Null
    Invoke-Recorded $winget $listArguments | Out-Null
    if (-not (Get-Item -LiteralPath $aliasPath -Force -ErrorAction SilentlyContinue)) { throw 'Installed portable alias is missing.' }
    # WinGet changes the persisted user PATH, not this PowerShell process.
    $env:PATH = "$(Split-Path $aliasPath);$originalPath"
    $installedCommand = (Get-Command mods -CommandType Application -ErrorAction Stop).Source
    if ($installedCommand -ne $aliasPath) { throw "Unexpected mods command: $installedCommand" }
    $versionReceipt = Invoke-Recorded $installedCommand @('--version')
    if ($versionReceipt.Stdout.Trim() -ne "mods $Version") {
        throw "Unexpected installed version: $($versionReceipt.Stdout)"
    }
    Invoke-Recorded $installedCommand @('--help') | Out-Null
}
catch {
    $failures.Add($_.ToString())
}
finally {
    $env:PATH = $originalPath
    if ($installAttempted) {
        try {
            $remaining = Invoke-Recorded $winget $listArguments @(0, $noApplications)
            if ($remaining.ExitCode -eq 0) {
                Invoke-Recorded $winget @('uninstall', '--id', $packageId, '--exact', '--version', $Version, '--scope', 'user', '--silent', '--accept-source-agreements', '--disable-interactivity', '--verbose-logs') | Out-Null
            }
            $after = Invoke-Recorded $winget $listArguments @(0, $noApplications)
            if ($after.ExitCode -ne $noApplications) { throw 'Mods remains installed after cleanup.' }
            if (Get-Item -LiteralPath $aliasPath -Force -ErrorAction SilentlyContinue) { throw 'Portable alias remains after cleanup.' }
        }
        catch { $failures.Add("Uninstall/alias cleanup: $_") }
    }
    if ($restoreLocalManifests) {
        try {
            Invoke-Recorded $winget @('settings', '--disable', 'LocalManifestFiles', '--disable-interactivity', '--verbose-logs') | Out-Null
            $restored = (Invoke-Recorded $winget @('settings', 'export', '--disable-interactivity')).Stdout | ConvertFrom-Json
            if ($restored.adminSettings.LocalManifestFiles -ne $false) { throw 'LocalManifestFiles was not restored.' }
        }
        catch { $failures.Add("Settings restoration: $_") }
    }
    try {
        if (-not (Test-Path -LiteralPath $nativeLogs)) { throw "WinGet diagnostic log directory missing: $nativeLogs" }
        Copy-Item -LiteralPath $nativeLogs -Destination (Join-Path $evidence 'native-logs') -Recurse -Force
    }
    catch { $failures.Add("Log retention: $_") }
    @{ version = $Version; passed = ($failures.Count -eq 0); failures = @($failures.ToArray()) } |
        ConvertTo-Json -Depth 4 | Set-Content (Join-Path $evidence 'result.json')
}
if ($failures.Count -gt 0) { throw ($failures -join "`n") }
