[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [ValidatePattern('^v[0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.-]+)?$')]
    [string]$Tag,
    [string]$Title = '',
    [switch]$Draft,
    [switch]$Prerelease,
    [string]$JavaHome = $env:JAVA_HOME
)

$ErrorActionPreference = 'Stop'
$repository = Split-Path -Parent $PSScriptRoot
$notes = Join-Path $repository 'build\release-notes.md'

foreach ($tool in @('git', 'git-cliff', 'gh')) {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
        throw "Required tool '$tool' was not found in PATH."
    }
}

Push-Location $repository
try {
    if (git status --porcelain) {
        throw 'The worktree must be clean before creating a release.'
    }
    gh auth status
    if ($LASTEXITCODE -ne 0) {
        throw 'GitHub CLI is not authenticated. Run gh auth login first.'
    }

    & (Join-Path $PSScriptRoot 'build.ps1') -Clean -JavaHome $JavaHome
    New-Item -ItemType Directory -Path (Split-Path -Parent $notes) -Force | Out-Null
    git cliff --unreleased --tag $Tag --strip header --output $notes
    if ($LASTEXITCODE -ne 0) {
        throw 'git-cliff failed to generate release notes.'
    }

    $artifacts = @(Get-ChildItem -LiteralPath (Join-Path $repository 'versions') -Recurse -Filter '*.jar' |
        Where-Object { $_.FullName -match '\\build\\libs\\' -and $_.Name -notlike '*-sources.jar' })
    if ($artifacts.Count -ne 8) {
        throw "Expected 8 release jars (4 versions x 2 loaders), found $($artifacts.Count)."
    }

    git rev-parse --verify --quiet "refs/tags/$Tag" | Out-Null
    if ($LASTEXITCODE -ne 0) {
        git tag -a $Tag -m "TrChat Mod $Tag"
        if ($LASTEXITCODE -ne 0) {
            throw "Unable to create tag $Tag."
        }
        git push origin $Tag
        if ($LASTEXITCODE -ne 0) {
            throw "Unable to push tag $Tag."
        }
    }

    $releaseTitle = if ($Title) { $Title } else { "TrChat Mod $Tag" }
    $arguments = @(
        'release', 'create', $Tag,
        '--verify-tag', '--title', $releaseTitle, '--notes-file', $notes,
        '--fail-on-no-commits'
    )
    if ($Draft) { $arguments += '--draft' }
    if ($Prerelease) { $arguments += '--prerelease' }
    $arguments += ($artifacts | ForEach-Object { $_.FullName })
    gh @arguments
    if ($LASTEXITCODE -ne 0) {
        throw 'GitHub release creation failed.'
    }
}
finally {
    Pop-Location
}