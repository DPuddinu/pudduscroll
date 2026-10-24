param(
  [Parameter(Mandatory = $true)][string]$Note,
  [string]$GenerationId
)

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

if ([string]::IsNullOrWhiteSpace($Note)) {
  [Console]::Error.WriteLine("A design note is required.")
  exit 1
}

$root = Resolve-WindowsPath (Get-Location).Path
$previous = $ErrorActionPreference
$ErrorActionPreference = "Continue"
Push-Location -LiteralPath $root
try {
  $gitDir = & git rev-parse --absolute-git-dir 2>$null
  $code = $LASTEXITCODE
} finally {
  Pop-Location
  $ErrorActionPreference = $previous
}
if ($code -ne 0 -or [string]::IsNullOrWhiteSpace([string]$gitDir)) {
  [Console]::Error.WriteLine("git rev-parse failed")
  exit 1
}
$gitDir = ((@($gitDir) | Select-Object -Last 1).ToString().Trim())

if ([string]::IsNullOrWhiteSpace($GenerationId)) {
  $pendingPath = Join-Path $gitDir "pudduscroll-design-pending"
  if (Test-Path -LiteralPath $pendingPath) {
    $GenerationId = ([System.IO.File]::ReadAllText($pendingPath)).Trim()
  }
}
if ([string]::IsNullOrWhiteSpace($GenerationId) -or $GenerationId -match "[\r\n]") {
  [Console]::Error.WriteLine("Missing generation id. Pass -GenerationId from the design gate.")
  exit 1
}

$stampPath = Join-Path $gitDir "pudduscroll-design-review"
$utf8 = New-Object System.Text.UTF8Encoding $false
[System.IO.File]::WriteAllText($stampPath, ($GenerationId.Trim() + "`n" + $Note.Trim()), $utf8)
[Console]::Out.WriteLine($GenerationId.Trim())