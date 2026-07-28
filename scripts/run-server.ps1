[CmdletBinding()]
param(
    [switch]$AcceptEula,
    [string]$JavaHome = 'C:\Program Files\Microsoft\jdk-21.0.10.7-hotspot'
)

$ErrorActionPreference = 'Stop'
$repository = Split-Path -Parent $PSScriptRoot
$runDirectory = Join-Path $repository 'run'
$javaExecutable = Join-Path $JavaHome 'bin\java.exe'

if (-not (Test-Path -LiteralPath $javaExecutable)) {
    throw "Java 21 was not found at '$JavaHome'. Pass -JavaHome with a Java 21 JDK path."
}

if ($AcceptEula) {
    New-Item -ItemType Directory -Path $runDirectory -Force | Out-Null
    Set-Content -LiteralPath (Join-Path $runDirectory 'eula.txt') -Value 'eula=true' -Encoding ascii
}

$previousJavaHome = $env:JAVA_HOME
try {
    $env:JAVA_HOME = $JavaHome
    & (Join-Path $repository 'gradlew.bat') 'runServer' '--no-configuration-cache' '--console=plain'
    exit $LASTEXITCODE
}
finally {
    $env:JAVA_HOME = $previousJavaHome
}
