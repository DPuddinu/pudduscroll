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

$raw = [Console]::In.ReadToEnd()
if ([string]::IsNullOrWhiteSpace($raw)) {
  Write-Allow
}

$hook = $raw | ConvertFrom-Json
$command = [string]$hook.command
if ($command -notmatch '(^|[\r\n]|[;&|]+\s*)git(\.exe)?(\s+\S+)*\s+commit(?=$|[\s;&|])') {
  Write-Allow
}

$cwd = [string]$hook.cwd
if ([string]::IsNullOrWhiteSpace($cwd) -and $hook.workspace_roots) {
  $cwd = [string]$hook.workspace_roots[0]
}
if ([string]::IsNullOrWhiteSpace($cwd)) {
  $cwd = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
}
$cwd = Resolve-WindowsPath $cwd

Push-Location -LiteralPath $cwd
try {
  $tree = (& git write-tree 2>$null)
  if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($tree)) {
    Write-Deny "Commit blocked until the staged changes are reviewed." "git write-tree failed, so the staged tree could not be checked. Fix the repository state, review the staged diff, then run powershell -NoProfile -File .cursor/hooks/mark-reviewed.ps1 and commit again."
  }
  $tree = $tree.Trim()

  $gitDir = (& git rev-parse --absolute-git-dir 2>$null)
  if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($gitDir)) {
    Write-Deny "Commit blocked until the staged changes are reviewed." "git rev-parse failed, so the review stamp could not be read. Review the staged diff, then run powershell -NoProfile -File .cursor/hooks/mark-reviewed.ps1 and commit again."
  }
  $stampPath = Join-Path $gitDir.Trim() "pudduscroll-commit-review"
  $stamp = ""
  if (Test-Path -LiteralPath $stampPath) {
    $stamp = ([System.IO.File]::ReadAllText($stampPath)).Trim()
  }
  if ($stamp -eq $tree) {
    Write-Allow
  }

  Write-Deny "Commit blocked until the staged changes are reviewed." @"
Read git diff --cached and report bugs, KISS violations, file splits, and edge cases in the reply.
Keep it simple: no extra types, parameters, or files for a need this change does not have. Split a file only when it has two unrelated reasons to change. too_many_lines stays allowed; do not split to hit a line count.
For core behavior, extend crates/core/src/tests.rs.
Fix format and Clippy before retrying: cargo fmt --all, then cargo clippy --workspace --all-targets -- -D warnings.
Stage fixes, run powershell -NoProfile -File .cursor/hooks/mark-reviewed.ps1, then commit again. Do not use --no-verify.
"@.Trim()
}
finally {
  Pop-Location
}
