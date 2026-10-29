# Real local Git pushes with a mocked GitHub CLI; no GitHub repositories change.
$ErrorActionPreference = 'Stop'
$helper = (Resolve-Path (Join-Path $PSScriptRoot '../publish_to_github.ps1')).Path
$originalDirectory = Get-Location
$work = Join-Path ([IO.Path]::GetTempPath()) ('radar-publish-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path $work | Out-Null
function global:gh {
    $global:LASTEXITCODE = 0
    if ($args[0] -eq 'repo' -and $args[1] -eq 'view' -and $global:radarTestNewRepository) {
        $global:LASTEXITCODE = 1
    }
}
try {
    foreach ($scenario in @('wrong-origin','wrong-pushurl','new-repository')) {
        $case = Join-Path $work $scenario
        New-Item -ItemType Directory -Path "$case/checkout" -Force | Out-Null
        & git init --bare --quiet "$case/target.git"
        if ($LASTEXITCODE -ne 0) { throw 'Could not create local target' }
        & git init --bare --quiet "$case/wrong.git"
        if ($LASTEXITCODE -ne 0) { throw 'Could not create local decoy' }
        Set-Location "$case/checkout"
        & git init --quiet -b main
        & git config user.name 'Radar publish test'
        & git config user.email 'radar-test@example.invalid'
        Copy-Item $helper './publish_to_github.ps1'
        & git add .
        & git commit --quiet -m 'Publication fixture'
        if ($LASTEXITCODE -ne 0) { throw 'Could not create publication fixture' }
        $expected = & git rev-parse HEAD
        $target = ([Uri]::new((Resolve-Path "$case/target.git").Path)).AbsoluteUri
        & git config "url.$target.insteadOf" 'https://github.com/radar-publish-smoke/target.git'
        & git remote add origin "$case/wrong.git"
        if ($scenario -eq 'wrong-pushurl') {
            & git remote set-url origin 'https://github.com/radar-publish-smoke/target.git'
            & git config --add remote.origin.pushurl "$case/wrong.git"
            & git config --add remote.origin.pushurl "$case/wrong.git"
        }
        $global:radarTestNewRepository = $scenario -eq 'new-repository'
        & './publish_to_github.ps1' -Owner 'radar-publish-smoke' -Repository 'target'
        $actual = & git --git-dir="$case/target.git" rev-parse refs/heads/main
        if ($LASTEXITCODE -ne 0 -or $actual -ne $expected) { throw "Wrong publication destination: $scenario" }
        & git --git-dir="$case/wrong.git" show-ref --quiet refs/heads/main
        if ($LASTEXITCODE -ne 1) { throw "Decoy repository was modified: $scenario" }
        Write-Host "PASS: publication destination ($scenario)"
    }
} finally {
    Set-Location $originalDirectory
    Remove-Item Function:gh -ErrorAction SilentlyContinue
    Remove-Variable radarTestNewRepository -Scope Global -ErrorAction SilentlyContinue
    Remove-Item $work -Recurse -Force
}
