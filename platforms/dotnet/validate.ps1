param(
  [Parameter(Mandatory=$true)][ValidateSet("win-x64")][string]$Rid,
  [Parameter(Mandatory=$true)][string]$Record,
  [Parameter(Mandatory=$true)][string]$Package,
  [Parameter(Mandatory=$true)][string]$Manifest
)
$ErrorActionPreference = "Stop"
python (Join-Path $PSScriptRoot "packaging/package.py") validate --rid $Rid --record $Record --package $Package --manifest $Manifest
exit $LASTEXITCODE
