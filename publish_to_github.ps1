# Create GitHub repository and push the source once (GitHub CLI required).
# Example: powershell -ExecutionPolicy Bypass -File .\publish_to_github.ps1
param(
    [string]$Repository = 'radar-cs',
    [string]$Owner = 'frd21313123123',
    [switch]$Public
)
$ErrorActionPreference = 'Stop'
if ($Owner -notmatch '^[A-Za-z0-9][A-Za-z0-9-]*$' -or $Repository -notmatch '^[A-Za-z0-9_.-]+$' -or $Repository -in @('.', '..')) {
    throw 'Invalid GitHub owner or repository name'
}
Set-Location $PSScriptRoot
foreach ($tool in @('git', 'gh')) {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
        throw "Missing $tool. Install Git for Windows (https://git-scm.com/downloads/win) and GitHub CLI (https://cli.github.com/)."
    }
}
$authenticated = $false
try {
    & gh auth status 2>$null | Out-Null
    $authenticated = ($LASTEXITCODE -eq 0)
} catch { $authenticated = $false }
if (-not $authenticated) {
    Write-Host 'Opening GitHub authentication...' -ForegroundColor Cyan
    & gh auth login
    if ($LASTEXITCODE -ne 0) { throw 'GitHub login failed' }
}
$fullName = "$Owner/$Repository"
if (-not (Test-Path '.git')) {
    & git init -b main
    if ($LASTEXITCODE -ne 0) { throw 'git init failed' }
}
& git add -A
if ($LASTEXITCODE -ne 0) { throw 'git add failed' }
& git diff --cached --quiet
if ($LASTEXITCODE -ne 0) {
    & git commit -m 'Initial CAS Radar Rust with Windows auto-release workflow'
    if ($LASTEXITCODE -ne 0) {
        throw 'Commit failed. Set user identity: git config --global user.name "Your Name" and git config --global user.email "you@example.com"'
    }
}
$exists = $false
try {
    & gh repo view $fullName --json name 2>$null | Out-Null
    $exists = ($LASTEXITCODE -eq 0)
} catch { $exists = $false }
if (-not $exists) {
    $visibility = if ($Public) { '--public' } else { '--private' }
    Write-Host "Creating $fullName ($visibility)" -ForegroundColor Cyan
    & gh repo create $fullName $visibility `
        --description 'CAS External Radar in Rust: native Win32 mini-map and auto Windows releases'
    if ($LASTEXITCODE -ne 0) { throw 'gh repo create failed' }
} else {
    Write-Host "Repository exists: $fullName" -ForegroundColor Yellow
}
# Retain HTTPS/SSH authentication routes only when they address the requested
# repository. Replace all URL entries so extra push destinations cannot survive.
$requested = '^(?:https://(?:[^/@]+@)?github\.com/|git@github\.com:|ssh://git@github\.com/)' + [regex]::Escape($fullName) + '(?:\.git)?/?$'
$originUrls = @(& git config --get-all remote.origin.url)
$pushUrls = @(& git config --get-all remote.origin.pushurl)
$remoteUrl = $originUrls | Where-Object { $_ -match $requested } | Select-Object -First 1
if (!$remoteUrl) { $remoteUrl = "https://github.com/$fullName.git" }
$pushUrl = $pushUrls | Where-Object { $_ -match $requested } | Select-Object -First 1
if (!$pushUrl) { $pushUrl = $remoteUrl }
if ($originUrls.Count -gt 0) { & git config --replace-all remote.origin.url $remoteUrl }
else { & git remote add origin $remoteUrl }
if ($LASTEXITCODE -ne 0) { throw 'Could not configure the requested GitHub remote' }
& git config --replace-all remote.origin.pushurl $pushUrl
if ($LASTEXITCODE -ne 0) { throw 'Could not configure the requested push destination' }
& git branch -M main
if ($LASTEXITCODE -ne 0) { throw 'Could not select main branch' }
& git push -u origin main
if ($LASTEXITCODE -ne 0) { throw 'git push failed' }
Write-Host ''
Write-Host "Repository: https://github.com/$fullName" -ForegroundColor Green
Write-Host "Automatic builds: https://github.com/$fullName/actions" -ForegroundColor Green
Write-Host "Releases: https://github.com/$fullName/releases" -ForegroundColor Green
Write-Host 'The first GitHub Actions run will publish EXE + ZIP to Releases if tests/build pass.'
