[CmdletBinding()]
param(
    [Parameter(Mandatory)] [string]$InstallerPath,
    [switch]$Execute,
    [string]$PreviousInstallerPath = ""
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$installer = (Resolve-Path -LiteralPath $InstallerPath).Path
if ([System.IO.Path]::GetExtension($installer) -ne ".exe") {
    throw "Installer must be an .exe file."
}
$hash = (Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash
$signature = Get-AuthenticodeSignature -LiteralPath $installer
$metadata = [ordered]@{
    installer = $installer
    sizeBytes = (Get-Item -LiteralPath $installer).Length
    sha256 = $hash
    signatureStatus = $signature.Status.ToString()
    executionPerformed = [bool]$Execute
}
$productNames = @("Sharman", "AgentRelay")

if (-not $Execute) {
    $metadata | ConvertTo-Json -Depth 4
    if ($metadata.sizeBytes -le 0) { throw "Installer is empty." }
    exit 0
}

$installedProduct = Get-ChildItem -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall" -ErrorAction SilentlyContinue |
    Get-ItemProperty -ErrorAction SilentlyContinue |
    Where-Object { $_.DisplayName -in $productNames } |
    Select-Object -First 1
if ($null -ne $installedProduct) {
    throw "Installer execution was not started because Sharman or AgentRelay is already registered for this user. Use metadata-only mode or test on a clean Windows account."
}

$testRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("AgentRelayRcInstaller-" + [guid]::NewGuid().ToString("N"))
$installDirectory = Join-Path $testRoot "app"
$isolatedAppData = Join-Path $testRoot "appdata"
$isolatedLocalAppData = Join-Path $testRoot "localappdata"
$uninstaller = $null
New-Item -ItemType Directory -Force -Path $installDirectory,$isolatedAppData,$isolatedLocalAppData | Out-Null
$resolvedTemp = (Resolve-Path -LiteralPath ([System.IO.Path]::GetTempPath())).Path
$resolvedTest = (Resolve-Path -LiteralPath $testRoot).Path
if (-not $resolvedTest.StartsWith($resolvedTemp, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "Refusing installer smoke test outside the temporary directory."
}

function Invoke-IsolatedProcess {
    param(
        [string]$FilePath,
        [string[]]$Arguments,
        [int[]]$AllowedExitCodes = @(0)
    )
    $info = [System.Diagnostics.ProcessStartInfo]::new()
    $info.FileName = $FilePath
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.Environment["APPDATA"] = $isolatedAppData
    $info.Environment["LOCALAPPDATA"] = $isolatedLocalAppData
    foreach ($argument in $Arguments) { [void]$info.ArgumentList.Add($argument) }
    $process = [System.Diagnostics.Process]::Start($info)
    if (-not $process.WaitForExit(10 * 60 * 1000)) {
        $process.Kill($true)
        throw "Installer process timed out."
    }
    if ($process.ExitCode -notin $AllowedExitCodes) { throw "Installer process failed with exit code $($process.ExitCode)." }
}

try {
    Invoke-IsolatedProcess -FilePath $installer -Arguments @("/S", "/D=$installDirectory")
    $application = Get-ChildItem -LiteralPath $installDirectory -Filter "agentrelay-desktop.exe" -File -Recurse | Select-Object -First 1
    if ($null -eq $application) { throw "Fresh install did not produce agentrelay-desktop.exe." }
    $uninstaller = Get-ChildItem -LiteralPath $installDirectory -Filter "uninstall.exe" -File -Recurse | Select-Object -First 1
    if ($null -eq $uninstaller) { throw "Installed package did not contain uninstall.exe." }

    Invoke-IsolatedProcess -FilePath $installer -Arguments @("/S", "/D=$installDirectory")
    if (-not (Test-Path -LiteralPath $application.FullName)) { throw "Update install removed the application binary." }

    $registered = Get-ChildItem -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall" -ErrorAction SilentlyContinue |
        Get-ItemProperty -ErrorAction SilentlyContinue |
        Where-Object { $_.DisplayName -in $productNames } |
        Select-Object -First 1
    if ($null -eq $registered) { throw "Installed package did not register an uninstall entry." }
    $currentDisplayVersion = $registered.DisplayVersion
    $applicationHash = (Get-FileHash -LiteralPath $application.FullName -Algorithm SHA256).Hash
    $simulatedNewerVersion = "99.0.0"
    $downgradeFailure = $null
    try {
        Set-ItemProperty -LiteralPath $registered.PSPath -Name "DisplayVersion" -Value $simulatedNewerVersion
        Invoke-IsolatedProcess -FilePath $installer -Arguments @("/S", "/D=$installDirectory") -AllowedExitCodes @(0, 1, 2)
        $afterDowngrade = Get-ItemProperty -LiteralPath $registered.PSPath
        $afterDowngradeHash = (Get-FileHash -LiteralPath $application.FullName -Algorithm SHA256).Hash
        if ($afterDowngrade.DisplayVersion -ne $simulatedNewerVersion -or $afterDowngradeHash -ne $applicationHash) {
            $downgradeFailure = "version changed from $simulatedNewerVersion to $($afterDowngrade.DisplayVersion), binaryChanged=$($afterDowngradeHash -ne $applicationHash)"
        }
    }
    finally {
        if (Test-Path -LiteralPath $registered.PSPath) {
            Set-ItemProperty -LiteralPath $registered.PSPath -Name "DisplayVersion" -Value $currentDisplayVersion
        }
    }
    if ($null -ne $downgradeFailure) { throw "Downgrade protection failed: $downgradeFailure." }

    if (-not [string]::IsNullOrWhiteSpace($PreviousInstallerPath)) {
        $previous = (Resolve-Path -LiteralPath $PreviousInstallerPath).Path
        $beforePreviousHash = (Get-FileHash -LiteralPath $application.FullName -Algorithm SHA256).Hash
        Invoke-IsolatedProcess -FilePath $previous -Arguments @("/S", "/D=$installDirectory") -AllowedExitCodes @(0, 1, 2)
        $registered = Get-ChildItem -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall" -ErrorAction SilentlyContinue |
            Get-ItemProperty -ErrorAction SilentlyContinue |
            Where-Object { $_.DisplayName -in $productNames } |
            Select-Object -First 1
        $afterPreviousHash = (Get-FileHash -LiteralPath $application.FullName -Algorithm SHA256).Hash
        if ($null -eq $registered -or $registered.DisplayVersion -ne $currentDisplayVersion -or $afterPreviousHash -ne $beforePreviousHash) {
            throw "Previous-version rejection failed: version=$($registered.DisplayVersion), binaryChanged=$($afterPreviousHash -ne $beforePreviousHash)."
        }
    }

    $memoryDirectory = Join-Path $isolatedAppData "com.agentrelay.desktop"
    New-Item -ItemType Directory -Force -Path $memoryDirectory | Out-Null
    Set-Content -LiteralPath (Join-Path $memoryDirectory "preserve-me.txt") -Value "RC installer preservation sentinel" -Encoding utf8
    Invoke-IsolatedProcess -FilePath $uninstaller.FullName -Arguments @("/S")
    $deleteDeadline = [DateTime]::UtcNow.AddSeconds(15)
    while ((Test-Path -LiteralPath $application.FullName) -and [DateTime]::UtcNow -lt $deleteDeadline) {
        [System.Threading.Thread]::Sleep(200)
    }
    if (Test-Path -LiteralPath $application.FullName) { throw "Uninstall left the application binary behind." }
    if (-not (Test-Path -LiteralPath (Join-Path $memoryDirectory "preserve-me.txt"))) {
        throw "Silent uninstall did not preserve project memory."
    }
    $metadata.freshInstall = "passed"
    $metadata.updateInstall = "passed"
    $metadata.downgradeRejection = "passed"
    $metadata.uninstall = "passed"
    $metadata.memoryPreservation = "passed"
    $metadata.previousInstallerAttempted = -not [string]::IsNullOrWhiteSpace($PreviousInstallerPath)
    $metadata | ConvertTo-Json -Depth 4
}
finally {
    if ($null -ne $uninstaller -and (Test-Path -LiteralPath $uninstaller.FullName)) {
        try {
            Invoke-IsolatedProcess -FilePath $uninstaller.FullName -Arguments @("/S") -AllowedExitCodes @(0, 1)
        }
        catch {
            Write-Warning "Isolated installer cleanup failed: $($_.Exception.Message)"
        }
    }
    if (Test-Path -LiteralPath $testRoot) {
        $confirmed = (Resolve-Path -LiteralPath $testRoot).Path
        if ($confirmed.StartsWith($resolvedTemp, [System.StringComparison]::OrdinalIgnoreCase) -and
            [System.IO.Path]::GetFileName($confirmed).StartsWith("AgentRelayRcInstaller-", [System.StringComparison]::Ordinal)) {
            Remove-Item -LiteralPath $confirmed -Recurse -Force
        }
    }
}
