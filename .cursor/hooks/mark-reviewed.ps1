$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $false

$tree = (& git write-tree 2>$null)
if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($tree)) {
  [Console]::Error.WriteLine("git write-tree failed")
  exit 1
}
$tree = $tree.Trim()

$gitDir = (& git rev-parse --absolute-git-dir 2>$null)
if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($gitDir)) {
  [Console]::Error.WriteLine("git rev-parse failed")
  exit 1
}

$stampPath = Join-Path $gitDir.Trim() "pudduscroll-commit-review"
$utf8 = New-Object System.Text.UTF8Encoding $false
[System.IO.File]::WriteAllText($stampPath, $tree, $utf8)
[Console]::Out.WriteLine($tree)
