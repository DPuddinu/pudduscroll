$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $false

function Write-Allow {
  [Console]::Out.WriteLine('{"permission":"allow"}')
  exit 0
}

function Write-Deny([string]$UserMessage, [string]$AgentMessage) {
  $payload = @{
    permission    = "deny"
    user_message  = $UserMessage
    agent_message = $AgentMessage
  }
  [Console]::Out.WriteLine(($payload | ConvertTo-Json -Compress))
  exit 0
}

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

function Get-RepoRoot([object]$Hook) {
  $root = $null
  if ($Hook.workspace_roots) { $root = [string]$Hook.workspace_roots[0] }
  if ([string]::IsNullOrWhiteSpace($root)) { $root = [string]$Hook.cwd }
  if ([string]::IsNullOrWhiteSpace($root)) {
    $root = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
  }
  return (Resolve-WindowsPath $root)
}

function Get-GitDir([string]$Root) {
  $previous = $ErrorActionPreference
  $ErrorActionPreference = "Continue"
  try {
    Push-Location -LiteralPath $Root
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
  if ([string]::IsNullOrWhiteSpace($raw)) { Write-Allow }
  $hook = $raw | ConvertFrom-Json
  $toolName = [string]$hook.tool_name
  if ($toolName -notmatch "^(Write|StrReplace|SearchReplace|search_replace|EditNotebook|ApplyPatch|Edit)$") {
    Write-Allow
  }

  $generationId = ([string]$hook.generation_id).Trim()
  $gitDir = Get-GitDir (Get-RepoRoot $hook)
  if ([string]::IsNullOrWhiteSpace($gitDir)) {
    Write-Deny "Implementation blocked until simplicity, reuse, and maintainability are checked." "The design gate could not find the git directory. Review the design, then run powershell -NoProfile -File .cursor/hooks/mark-design.ps1 -Note `"<summary>`" -GenerationId `"<id>`"."
  }

  $stamp = ""
  $stampPath = Join-Path $gitDir "pudduscroll-design-review"
  if (Test-Path -LiteralPath $stampPath) {
    $stamp = [System.IO.File]::ReadAllText($stampPath)
  }
  $stampId = ""
  $stampNote = ""
  if (-not [string]::IsNullOrWhiteSpace($stamp)) {
    $parts = @($stamp -split "\r?\n", 2)
    $stampId = $parts[0].Trim()
    if ($parts.Count -gt 1) { $stampNote = $parts[1].Trim() }
  }
  $pendingId = ""
  $pendingPath = Join-Path $gitDir "pudduscroll-design-pending"
  if (Test-Path -LiteralPath $pendingPath) {
    $pendingId = ([System.IO.File]::ReadAllText($pendingPath)).Trim()
  }
  $stampMatches = $stampId -eq $generationId -or (-not [string]::IsNullOrWhiteSpace($pendingId) -and $stampId -eq $pendingId)
  if (-not [string]::IsNullOrWhiteSpace($stampId) -and -not [string]::IsNullOrWhiteSpace($stampNote) -and $stampMatches) {
    Write-Allow
  }

  $idArg = $generationId
  if ([string]::IsNullOrWhiteSpace($idArg)) { $idArg = "<id>" }
  Write-Deny "Implementation blocked until simplicity, reuse, and maintainability are checked." @"
Report these four points in the reply before editing:
- Simpler: the smallest change that does this, and what you will not add.
- Reusable: the existing type or function to extend, or why new code is needed.
- Maintainable: the one place a later change to this behavior should land.
- Pattern: use one from https://refactoring.guru/design-patterns/rust only when it makes this change simpler or keeps a future change in one place. Name it, or say none fits. A direct function beats a pattern with one caller.
Core behavior stays in pudduscroll-core. Do not write source from the shell to skip this gate.
Then run powershell -NoProfile -File .cursor/hooks/mark-design.ps1 -Note "<one-line summary>" -GenerationId "$idArg" and edit again.
"@.Trim()
} catch {
  $payload = @{
    permission    = "deny"
    user_message  = "Implementation blocked until simplicity, reuse, and maintainability are checked."
    agent_message = ("Design gate failed: " + $_.Exception.Message)
  }
  [Console]::Out.WriteLine(($payload | ConvertTo-Json -Compress))
  exit 0
}