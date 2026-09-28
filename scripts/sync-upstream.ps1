[CmdletBinding()]
param(
    [switch]$MarkSynced,
    [string]$UpstreamUrl = 'https://github.com/TrPlugins/TrChat.git',
    [string]$Branch = 'v2'
)

$ErrorActionPreference = 'Stop'
$repository = Split-Path -Parent $PSScriptRoot
$stateFile = Join-Path $repository 'upstream-sync.properties'
$state = Get-Content -LiteralPath $stateFile |
    Where-Object { $_ -match '^\s*upstream_commit\s*=' } |
    Select-Object -First 1

if (-not $state) {
    throw "Missing upstream_commit in '$stateFile'."
}
$lastSynced = ($state -split '=', 2)[1].Trim()

Push-Location $repository
try {
    $remotes = @(git remote)
    if ($remotes -notcontains 'upstream') {
        git remote add upstream $UpstreamUrl
    }
    else {
        $remoteUrl = (git remote get-url upstream).Trim()
    }
    if ($remotes -contains 'upstream' -and $remoteUrl -ne $UpstreamUrl) {
        git remote set-url upstream $UpstreamUrl
    }

    $previousPreference = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    $fetchOutput = git fetch upstream $Branch 2>&1
    $fetchExitCode = $LASTEXITCODE
    $ErrorActionPreference = $previousPreference
    $fetchOutput | ForEach-Object { Write-Host $_ }
    if ($fetchExitCode -ne 0) {
        throw 'Unable to fetch the TrChat upstream repository.'
    }

    $upstreamHead = (git rev-parse "upstream/$Branch").Trim()
    Write-Host "Last reviewed: $lastSynced"
    Write-Host "Upstream head: $upstreamHead"

    if ($lastSynced -eq $upstreamHead) {
        Write-Host 'The Redis compatibility baseline is current.'
        return
    }

    $paths = @(
        'project/common/src/main/java/me/arasple/mc/trchat/util/proxy/common',
        'project/common/src/main/kotlin/me/arasple/mc/trchat/util/proxy',
        'project/runtime-bukkit/src/main/kotlin/me/arasple/mc/trchat/api/impl/BukkitProxyManager.kt',
        'project/runtime-bukkit/src/main/kotlin/me/arasple/mc/trchat/module/internal/proxy',
        'project/runtime-bukkit/src/main/resources/channels',
        'project/runtime-bukkit/src/main/resources/settings.yml'
    )

    Write-Host 'Relevant upstream commits:'
    git log --oneline "$lastSynced..upstream/$Branch" -- @paths

    Write-Host 'Relevant upstream diff:'
    git diff $lastSynced "upstream/$Branch" -- @paths

    if ($MarkSynced) {
        Set-Content -LiteralPath $stateFile -Value @(
            '# Last TrChat Bukkit v2 commit reviewed for Redis/chat wire compatibility.'
            "upstream_commit=$upstreamHead"
        ) -Encoding utf8
        Write-Host "Marked $upstreamHead as reviewed. Commit this file with the corresponding port."
    }
    else {
        Write-Host 'Review and port relevant changes, run the build, then rerun with -MarkSynced.'
    }
}
finally {
    Pop-Location
}
