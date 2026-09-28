[CmdletBinding()]
param(
    [switch]$AcceptEula,
    [string]$JavaHome = $env:JAVA_HOME
)

$ErrorActionPreference = 'Stop'
$repository = Split-Path -Parent $PSScriptRoot
$runDirectory = Join-Path $repository 'run'
$javaExecutable = if ($JavaHome) {
    Join-Path $JavaHome 'bin\java.exe'
}
else {
    (Get-Command java -ErrorAction SilentlyContinue).Source
}

if (-not $javaExecutable -or -not (Test-Path -LiteralPath $javaExecutable)) {
    throw 'Java 21 was not found. Set JAVA_HOME or pass -JavaHome with a Java 21 JDK path.'
}

if ($AcceptEula) {
    New-Item -ItemType Directory -Path $runDirectory -Force | Out-Null
    Set-Content -LiteralPath (Join-Path $runDirectory 'eula.txt') -Value 'eula=true' -Encoding ascii
}

$previousJavaHome = $env:JAVA_HOME
try {
    if ($JavaHome) {
        $env:JAVA_HOME = $JavaHome
    }
    & (Join-Path $repository 'gradlew.bat') 'runServer' '--no-configuration-cache' '--console=plain'
    exit $LASTEXITCODE
}
finally {
    $env:JAVA_HOME = $previousJavaHome
}
