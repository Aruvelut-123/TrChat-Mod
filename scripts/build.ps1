[CmdletBinding()]
param(
    [switch]$Clean,
    [string]$JavaHome = $env:JAVA_HOME
)

$ErrorActionPreference = 'Stop'
$repository = Split-Path -Parent $PSScriptRoot
$wrapper = Join-Path $repository 'gradlew.bat'
$javaExecutable = if ($JavaHome) {
    Join-Path $JavaHome 'bin\java.exe'
}
else {
    (Get-Command java -ErrorAction SilentlyContinue).Source
}

if (-not $javaExecutable -or -not (Test-Path -LiteralPath $javaExecutable)) {
    throw 'Java 21 was not found. Set JAVA_HOME or pass -JavaHome with a Java 21 JDK path.'
}

$previousJavaHome = $env:JAVA_HOME
try {
    if ($JavaHome) {
        $env:JAVA_HOME = $JavaHome
    }
    if ($Clean) {
        & $wrapper 'clean' 'build' '--no-configuration-cache' '--console=plain'
    }
    else {
        & $wrapper 'build' '--no-configuration-cache' '--console=plain'
    }
    if ($LASTEXITCODE -ne 0) {
        throw "Gradle build failed with exit code $LASTEXITCODE."
    }

    Get-ChildItem -LiteralPath (Join-Path $repository 'build\libs') -Filter 'trchat_neoforge-*.jar' |
        Where-Object { $_.Name -notlike '*-sources.jar' } |
        Select-Object FullName, Length, LastWriteTime
}
finally {
    $env:JAVA_HOME = $previousJavaHome
}
