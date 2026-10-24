$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $false

function Resolve-WindowsPath([string]$Path) {
  if ([string]::IsNullOrWhiteSpace($Path)) { return $Path }
  $Path = $Path.Trim().Trim('"')
  if ($Path -match '^/([A-Za-z]):[/\\](.*)$') {
    $Path = "{0}:\{1}" -f $Matches[1], ($Matches[2] -replace '/', '\')
  } elseif ($Path -match '^/([A-Za-z])/(.*)$') {
    $Path = "{0}:\{1}" -f $Matches[1], ($Matches[2] -replace '/', '\')
  } else {
    $Path = $Path -replace '/', '\'
  }
  return [System.IO.Path]::GetFullPath($Path)
}

function Get-GitDir([object]$Hook) {
  $root = $null
  if ($Hook -and $Hook.workspace_roots) { $root = [string]$Hook.workspace_roots[0] }
  if ([string]::IsNullOrWhiteSpace($root) -and $Hook) { $root = [string]$Hook.cwd }
  if ([string]::IsNullOrWhiteSpace($root)) {
    $root = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
  }
  $root = Resolve-WindowsPath $root
  $previous = $ErrorActionPreference
  $ErrorActionPreference = "Continue"
  try {
    Push-Location -LiteralPath $root
  } catch {
    return $null
  }
  try {
    $dir = & git rev-parse --absolute-git-dir 2>$null
    if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace([string]$dir)) { return $null }
    return ((@($dir) | Select-Object -Last 1).ToString().Trim())
  } finally {
    Pop-Location
    $ErrorActionPreference = $previous
  }
}

try {
  $raw = [Console]::In.ReadToEnd()
  $hook = $null
  if (-not [string]::IsNullOrWhiteSpace($raw)) { $hook = $raw | ConvertFrom-Json }
  $gitDir = Get-GitDir $hook
  if ($gitDir) {
    $reviewPath = Join-Path $gitDir "pudduscroll-design-review"
    $pendingPath = Join-Path $gitDir "pudduscroll-design-pending"
    if (Test-Path -LiteralPath $reviewPath) { Remove-Item -LiteralPath $reviewPath -Force }
    $generationId = ""
    if ($hook) { $generationId = ([string]$hook.generation_id).Trim() }
    if ([string]::IsNullOrWhiteSpace($generationId) -or $generationId -match "[\r\n]") {
      if (Test-Path -LiteralPath $pendingPath) { Remove-Item -LiteralPath $pendingPath -Force }
    } else {
      $utf8 = New-Object System.Text.UTF8Encoding $false
      [System.IO.File]::WriteAllText($pendingPath, $generationId, $utf8)
    }
  }
} catch {
  [Console]::Error.WriteLine($_.Exception.Message)
}

[Console]::Out.WriteLine('{"continue":true}')
exit 0