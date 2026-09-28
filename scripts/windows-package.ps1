# Shared production ZIP formatter: explicit children avoid Windows Shell's ./ bug.
function New-RuntimeZip([string]$Directory, [string]$Archive) {
  $children = @(Get-ChildItem -LiteralPath $Directory -Force | Sort-Object Name | ForEach-Object Name)
  if (-not $children.Count) { throw 'Runtime directory is empty' }
  tar -a -cf $Archive -C $Directory -- @children
  if ($LASTEXITCODE -ne 0) { throw 'Distribution ZIP failed' }
}
