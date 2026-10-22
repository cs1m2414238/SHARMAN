param([string]$NativeExecutable)

$ErrorActionPreference = 'Stop'
$relayRoot = Split-Path -Parent $PSScriptRoot
if (-not $NativeExecutable) { $NativeExecutable = Join-Path $relayRoot 'target\debug\agentrelay-desktop.exe' }
$NativeExecutable = (Resolve-Path -LiteralPath $NativeExecutable).Path
$env:SHARMAN_NATIVE_EXECUTABLE = $NativeExecutable
if (Get-NetTCPConnection -State Listen -LocalPort 4179,4180 -ErrorAction SilentlyContinue) { throw 'Ports 4179 or 4180 are already in use. Stop the previous demo before launching this validation session.' }
$probe = '{"tool":"snapshot"}' | & $NativeExecutable --personal-bridge | ConvertFrom-Json
if (-not $probe.ok) { throw 'Rebuild the native app with cargo build -p agentrelay-desktop before starting validation.' }
$nodeExecutable = (Get-Command node -ErrorAction Stop).Source
$logDirectory = Join-Path $relayRoot 'target\qa\alexa-local'
New-Item -ItemType Directory -Path $logDirectory -Force | Out-Null
$mcpProcess = $null
$webProcess = $null
try {
    $mcpProcess = Start-Process -FilePath $nodeExecutable -ArgumentList 'dist/server.js' -WorkingDirectory (Join-Path $relayRoot 'sharman-mcp') -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $logDirectory 'mcp.stdout.log') -RedirectStandardError (Join-Path $logDirectory 'mcp.stderr.log')
    $webProcess = Start-Process -FilePath $nodeExecutable -ArgumentList @('node_modules/vite/bin/vite.js', '--host', '127.0.0.1', '--port', '4180', '--strictPort') -WorkingDirectory (Join-Path $relayRoot 'sharman-web') -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $logDirectory 'web.stdout.log') -RedirectStandardError (Join-Path $logDirectory 'web.stderr.log')
    Write-Host 'Alexa console: http://127.0.0.1:4180/alexa'
    Write-Host 'MCP endpoint:  http://127.0.0.1:4179/mcp'
    Write-Host 'Native tasks and reminders are connected. Keep the SHARMAN desktop app running for notifications.'
    Write-Host 'Press Enter to stop these two validation processes.'
    Read-Host | Out-Null
} finally {
    foreach ($validationProcess in @($webProcess, $mcpProcess)) {
        if ($validationProcess -and -not $validationProcess.HasExited) { Stop-Process -Id $validationProcess.Id }
    }
}
