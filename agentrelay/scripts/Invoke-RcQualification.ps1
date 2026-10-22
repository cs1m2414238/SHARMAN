[CmdletBinding()]
param(
    [string]$ResultsDirectory = "",
    [switch]$SkipPerformance,
    [switch]$SkipDesktopBundle,
    [switch]$ReuseDesktopBundle,
    [switch]$ExecuteInstallerSmoke,
    [string]$PreviousInstallerPath = ""
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
Set-StrictMode -Version Latest

$repositoryRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot "..")).Path
if ([string]::IsNullOrWhiteSpace($ResultsDirectory)) {
    $ResultsDirectory = Join-Path $repositoryRoot "target\rc-evidence"
}
$runId = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssZ")
$runDirectory = Join-Path $ResultsDirectory $runId
New-Item -ItemType Directory -Force -Path $runDirectory | Out-Null

function Invoke-RcStep {
    param(
        [Parameter(Mandatory)] [string]$Name,
        [Parameter(Mandatory)] [string]$FilePath,
        [Parameter(Mandatory)] [string[]]$Arguments,
        [Parameter(Mandatory)] [string]$WorkingDirectory,
        [int]$TimeoutMinutes = 45
    )

    $command = Get-Command $FilePath -ErrorAction Stop
    $processInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $processInfo.FileName = $command.Source
    $processInfo.WorkingDirectory = $WorkingDirectory
    $processInfo.UseShellExecute = $false
    $processInfo.CreateNoWindow = $true
    $processInfo.RedirectStandardOutput = $true
    $processInfo.RedirectStandardError = $true
    foreach ($argument in $Arguments) {
        [void]$processInfo.ArgumentList.Add($argument)
    }

    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $processInfo
    $startedAt = (Get-Date).ToUniversalTime()
    $clock = [System.Diagnostics.Stopwatch]::StartNew()
    [void]$process.Start()
    $stdout = $process.StandardOutput.ReadToEndAsync()
    $stderr = $process.StandardError.ReadToEndAsync()
    $completed = $process.WaitForExit($TimeoutMinutes * 60 * 1000)
    if (-not $completed) {
        $process.Kill($true)
        $process.WaitForExit()
    }
    $clock.Stop()
    $stdoutText = $stdout.GetAwaiter().GetResult()
    $stderrText = $stderr.GetAwaiter().GetResult()
    $safeName = $Name.ToLowerInvariant() -replace '[^a-z0-9]+', '-'
    Set-Content -LiteralPath (Join-Path $runDirectory "$safeName.stdout.log") -Value $stdoutText -Encoding utf8
    Set-Content -LiteralPath (Join-Path $runDirectory "$safeName.stderr.log") -Value $stderrText -Encoding utf8
    $exitCode = if ($completed) { $process.ExitCode } else { -1 }

    [pscustomobject]@{
        name = $Name
        command = "$FilePath $($Arguments -join ' ')"
        status = if ($completed -and $exitCode -eq 0) { "passed" } elseif (-not $completed) { "timedOut" } else { "failed" }
        exitCode = $exitCode
        durationMs = $clock.ElapsedMilliseconds
        startedAt = $startedAt.ToString("o")
    }
}

$steps = [System.Collections.Generic.List[object]]::new()
$steps.Add((Invoke-RcStep -Name "Rust format" -FilePath "cargo" -Arguments @("fmt", "--all", "--", "--check") -WorkingDirectory $repositoryRoot))
$steps.Add((Invoke-RcStep -Name "Rust clippy" -FilePath "cargo" -Arguments @("clippy", "--workspace", "--all-targets", "--", "-D", "warnings") -WorkingDirectory $repositoryRoot))
$steps.Add((Invoke-RcStep -Name "Rust tests" -FilePath "cargo" -Arguments @("test", "--workspace") -WorkingDirectory $repositoryRoot))
$steps.Add((Invoke-RcStep -Name "Frontend build" -FilePath "npm.cmd" -Arguments @("run", "build") -WorkingDirectory (Join-Path $repositoryRoot "frontend")))

if (-not $SkipPerformance) {
    $steps.Add((Invoke-RcStep -Name "Performance baseline" -FilePath "cargo" -Arguments @("test", "-p", "agentrelay-core", "release_candidate_event_query_baseline", "--", "--ignored", "--nocapture") -WorkingDirectory $repositoryRoot))
}

$installer = $null
if (-not $SkipDesktopBundle) {
    if (-not $ReuseDesktopBundle) {
        $steps.Add((Invoke-RcStep -Name "Tauri release bundle" -FilePath "npm.cmd" -Arguments @("run", "tauri", "--", "build") -WorkingDirectory (Join-Path $repositoryRoot "frontend") -TimeoutMinutes 60))
    }
    $installer = Get-ChildItem -LiteralPath (Join-Path $repositoryRoot "target\release\bundle\nsis") -Filter "*-setup.exe" -File -ErrorAction SilentlyContinue |
        Sort-Object LastWriteTimeUtc -Descending |
        Select-Object -First 1
    if ($null -ne $installer) {
        $installerArguments = @("-NoProfile", "-File", (Join-Path $repositoryRoot "scripts\Test-Installer.ps1"), "-InstallerPath", $installer.FullName)
        if ($ExecuteInstallerSmoke) { $installerArguments += "-Execute" }
        if (-not [string]::IsNullOrWhiteSpace($PreviousInstallerPath)) {
            $installerArguments += @("-PreviousInstallerPath", $PreviousInstallerPath)
        }
        $steps.Add((Invoke-RcStep -Name "Installer smoke" -FilePath "pwsh" -Arguments $installerArguments -WorkingDirectory $repositoryRoot -TimeoutMinutes 20))
    } else {
        $steps.Add([pscustomobject]@{
            name = "Installer smoke"; command = "Test-Installer.ps1"; status = "failed"
            exitCode = 2; durationMs = 0; startedAt = (Get-Date).ToUniversalTime().ToString("o")
        })
    }
}

$os = Get-CimInstance Win32_OperatingSystem
$summary = [ordered]@{
    schemaVersion = 1
    release = "0.20.0-rc.2"
    runId = $runId
    startedFrom = $repositoryRoot
    completedAt = (Get-Date).ToUniversalTime().ToString("o")
    machine = [ordered]@{
        os = $os.Caption
        osVersion = $os.Version
        architecture = $os.OSArchitecture
        powershell = $PSVersionTable.PSVersion.ToString()
        rust = (& rustc --version)
        node = (& node --version)
    }
    installer = if ($null -eq $installer) { $null } else { $installer.FullName }
    steps = $steps
    passed = -not ($steps | Where-Object { $_.status -ne "passed" })
}

$summary | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $runDirectory "summary.json") -Encoding utf8
$markdown = [System.Collections.Generic.List[string]]::new()
$markdown.Add("# AgentRelay RC qualification — $runId")
$markdown.Add("")
$markdown.Add("Host: $($summary.machine.os) $($summary.machine.osVersion) ($($summary.machine.architecture))")
$markdown.Add("")
$markdown.Add("| Gate | Result | Duration |")
$markdown.Add("| --- | --- | ---: |")
foreach ($step in $steps) {
    $markdown.Add("| $($step.name) | $($step.status) | $([math]::Round($step.durationMs / 1000, 2)) s |")
}
$markdown.Add("")
$markdown.Add("Overall: **$(if ($summary.passed) { 'PASSED' } else { 'FAILED' })**")
$markdown | Set-Content -LiteralPath (Join-Path $runDirectory "summary.md") -Encoding utf8

Write-Host "RC evidence: $runDirectory"
if (-not $summary.passed) {
    throw "One or more release-candidate qualification gates failed. See $runDirectory."
}
