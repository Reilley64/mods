# Run this script from an elevated Windows PowerShell.
$ErrorActionPreference = "Stop"
$feature = Get-WindowsOptionalFeature -Online -FeatureName Client-ProjFS
if ($feature.State -eq "Enabled") {
    Write-Host "Windows Projected File System is already enabled."
    exit 0
}

$result = Enable-WindowsOptionalFeature -Online -FeatureName Client-ProjFS -All -NoRestart
if ($result.RestartNeeded) {
    [Console]::Error.WriteLine("Client-ProjFS was enabled but Windows requires a restart. Restart, then run this script again.")
    exit 3010
}
$updated = Get-WindowsOptionalFeature -Online -FeatureName Client-ProjFS
Write-Host "Client-ProjFS state: $($updated.State)"
if ($updated.State -ne "Enabled") {
    [Console]::Error.WriteLine("Client-ProjFS is not Enabled after setup.")
    exit 1
}
exit 0
