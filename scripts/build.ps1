[CmdletBinding()]
param(
    [switch]$Clean,
    [string]$JavaHome = 'C:\Program Files\Microsoft\jdk-21.0.10.7-hotspot'
)

$ErrorActionPreference = 'Stop'
$repository = Split-Path -Parent $PSScriptRoot
$wrapper = Join-Path $repository 'gradlew.bat'
$javaExecutable = Join-Path $JavaHome 'bin\java.exe'

if (-not (Test-Path -LiteralPath $javaExecutable)) {
    throw "Java 21 was not found at '$JavaHome'. Pass -JavaHome with a Java 21 JDK path."
}

$previousJavaHome = $env:JAVA_HOME
try {
    $env:JAVA_HOME = $JavaHome
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
        Select-Object FullName, Length, LastWriteTime
}
finally {
    $env:JAVA_HOME = $previousJavaHome
}
