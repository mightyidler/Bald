param([string]$InstallerPath)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'ShortcutMetadata.ps1')
$repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
$config = Get-Content (Join-Path $repoRoot 'tauri.conf.json') -Raw | ConvertFrom-Json
if ($config.bundle.windows.nsis.installMode -ne 'currentUser') { throw 'This test requires currentUser installation.' }
if (-not $InstallerPath) { $InstallerPath = Join-Path $repoRoot "target\release\bundle\nsis\Bald_$($config.version)_x64-setup.exe" }
$InstallerPath = (Resolve-Path -LiteralPath $InstallerPath).Path
$generated = Join-Path $repoRoot 'target\release\nsis\x64'
$makensis = Join-Path $env:LOCALAPPDATA 'tauri\NSIS\makensis.exe'
$sevenZipCommand = Get-Command 7z -ErrorAction SilentlyContinue
$sevenZip = Join-Path $env:ProgramFiles '7-Zip\7z.exe'
if ($sevenZipCommand) { $sevenZip = $sevenZipCommand.Source }
$id = [guid]::NewGuid().ToString('N').Substring(0, 8)
$product = "Bald Verification $id"
$bundleId = "app.bald.verification.$id"
$manufacturer = "baldInstallerVerification$id"
$work = Join-Path $repoRoot "target\installer-verification\$id"
$installPath = Join-Path $env:LOCALAPPDATA "BaldVerification-$id"
$shortcutPath = Join-Path ([Environment]::GetFolderPath('Programs')) "$product.lnk"
$desktopPath = Join-Path ([Environment]::GetFolderPath('Desktop')) "$product.lnk"
$uninstallKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\$product"
$manufacturerKey = "HKCU:\Software\$manufacturer"
$settingsPath = Join-Path $env:APPDATA "$bundleId\verification.json"
$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$realShortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'Bald.lnk'
$realConfig = Join-Path $env:APPDATA 'Bald\Bald\config\config.json'
$realInstall = Join-Path $env:LOCALAPPDATA 'Bald'
$successfulShortcut = if (Test-Path -LiteralPath $realShortcut) { Get-ShortcutMetadata $realShortcut } else { $null }
$protectedHashes = @{}
$protectedState = @{}
foreach ($path in @($realShortcut, $realConfig, (Join-Path $realInstall 'bald.exe'), (Join-Path $realInstall 'icons\icon.ico'), (Join-Path $realInstall 'uninstall.exe'))) {
    if (Test-Path -LiteralPath $path) { $protectedHashes[$path] = (Get-FileHash -LiteralPath $path).Hash }
    $protectedState[$path] = if (Test-Path -LiteralPath $path) {
        @((Get-Item -LiteralPath $path).LastWriteTimeUtc.Ticks, (Get-Item -LiteralPath $path).Attributes.ToString(), (Get-Acl -LiteralPath $path).Sddl) -join '|'
    } else { 'absent' }
}
foreach ($path in @($installPath, $shortcutPath, $desktopPath, $uninstallKey, $manufacturerKey)) {
    if (Test-Path -LiteralPath $path) { throw "Test identity already exists: $path" }
}
New-Item -ItemType Directory -Path $work -Force | Out-Null
if (Test-Path -LiteralPath (Split-Path $settingsPath)) { throw 'Test settings directory already exists.' }
$payload = Join-Path $work 'payload'
& $sevenZip x $InstallerPath "-o$payload" 'bald.exe' 'icons\icon.ico' -y | Out-File (Join-Path $work 'extract.log')
if ($LASTEXITCODE -ne 0) { throw 'Could not extract the release installer.' }
Add-Type -Path (Join-Path $PSScriptRoot 'InstallerIconProbe.cs')
$payloadHash = (Get-FileHash (Join-Path $payload 'bald.exe')).Hash
$iconCount = [InstallerIconProbe]::Verify((Join-Path $payload 'bald.exe'), (Join-Path $repoRoot 'icons\icon.ico'))
if ([Diagnostics.FileVersionInfo]::GetVersionInfo((Join-Path $payload 'bald.exe')).ProductVersion -ne $config.version) { throw 'EXE version differs from installer configuration.' }
if ((Get-FileHash (Join-Path $payload 'icons\icon.ico')).Hash -ne (Get-FileHash (Join-Path $repoRoot 'icons\icon.ico')).Hash) { throw 'Installer icon resource differs from source.' }
Write-Output "PASS: release installer contains EXE and $iconCount matching icon sizes."

# Clone only the generated installer's identity, using the actual release payload.
# Exercise the installer lifecycle without uninstalling the user's BALD or launching watchers.
$script = Get-Content (Join-Path $generated 'installer.nsi') -Raw
$script = $script -replace '(?m)^!define PRODUCTNAME ".*"', "!define PRODUCTNAME `"$product`""
$script = $script -replace '(?m)^!define BUNDLEID ".*"', "!define BUNDLEID `"$bundleId`""
$script = $script -replace '(?m)^!define MANUFACTURER ".*"', "!define MANUFACTURER `"$manufacturer`""
$script = $script -replace '(?m)^!define MAINBINARYSRCPATH ".*"', "!define MAINBINARYSRCPATH `"$(Join-Path $payload 'bald.exe')`""
$script = $script -replace '(?m)^!define OUTFILE ".*"', "!define OUTFILE `"$(Join-Path $work 'setup.exe')`""
$script = $script.Replace((Join-Path $repoRoot 'icons\icon.ico'), (Join-Path $payload 'icons\icon.ico'))
[IO.File]::WriteAllText((Join-Path $work 'installer.nsi'), $script, (New-Object Text.UTF8Encoding($false)))
foreach ($name in @('utils.nsh', 'FileAssociation.nsh', 'English.nsh')) { Copy-Item (Join-Path $generated $name) (Join-Path $work $name) }
$olderScript = $script -replace '(?m)^!define VERSION ".*"', '!define VERSION "0.0.0"'
$olderScript = $olderScript -replace '(?m)^!define VERSIONWITHBUILD ".*"', '!define VERSIONWITHBUILD "0.0.0.0"'
$olderScript = $olderScript.Replace((Join-Path $work 'setup.exe'), (Join-Path $work 'setup-old.exe'))
[IO.File]::WriteAllText((Join-Path $work 'installer-old.nsi'), $olderScript, (New-Object Text.UTF8Encoding($false)))
$compileScripts = @('installer.nsi', 'installer-old.nsi')
# Select the upgrade page's first radio option without driving native UI.
$maintenanceScript = $script -replace '(?m)^  \$\{NSD_GetState\} \$R2 \$R1$', '  StrCpy $R1 1'
$maintenanceScript = $maintenanceScript.Replace((Join-Path $work 'setup.exe'), (Join-Path $work 'setup-maintenance.exe'))
[IO.File]::WriteAllText((Join-Path $work 'installer-maintenance.nsi'), $maintenanceScript, (New-Object Text.UTF8Encoding($false)))
$compileScripts += 'installer-maintenance.nsi'
Push-Location $work
try {
    foreach ($name in $compileScripts) {
        & $makensis /V2 $name | Out-File (Join-Path $work "$name.log")
        if ($LASTEXITCODE -ne 0) { throw 'Test installer compilation failed.' }
    }
}
finally { Pop-Location }

function Run-Installer([string]$arguments, [string]$file = 'setup.exe') {
    $process = Start-Process (Join-Path $work $file) -ArgumentList "$arguments /D=$installPath" -WindowStyle Hidden -PassThru
    if (-not $process.WaitForExit(60000)) { $process.Kill(); throw 'Installer timed out.' }
    if ($process.ExitCode -ne 0) { throw "Installer failed: $($process.ExitCode)" }
}
function Assert-Installed([string]$expectedVersion = $config.version, [bool]$referenceLink = $false) {
    $exe = Join-Path $installPath 'bald.exe'
    if (-not (Test-Path -LiteralPath $exe)) { throw 'Installed EXE missing.' }
    if ((Get-FileHash -LiteralPath $exe).Hash -ne $payloadHash) { throw 'Installed EXE differs from release payload.' }
    [void][InstallerIconProbe]::Verify($exe, (Join-Path $repoRoot 'icons\icon.ico'))
    $link = (New-Object -ComObject WScript.Shell).CreateShortcut($shortcutPath)
    if ($link.TargetPath -ne $exe -or $link.WorkingDirectory -ne $installPath -or $link.Arguments -ne '') { throw 'Shortcut target, directory or arguments incorrect.' }
    $expectedIcon = if ($referenceLink) { "$exe,0" } else { "$(Join-Path $installPath 'icons\icon.ico'),0" }
    if ($link.IconLocation -ne $expectedIcon) { throw 'Shortcut explicit icon reference differs from successful configuration.' }
    $item = (New-Object -ComObject Shell.Application).Namespace([Environment]::GetFolderPath('Programs')).ParseName("$product.lnk")
    $expectedId = if ($referenceLink) { '' } else { $bundleId }
    if ([string]$item.ExtendedProperty('System.AppUserModel.ID') -ne $expectedId) { throw 'Shortcut app identity changed or missing.' }
    $metadata = Get-ShortcutMetadata $shortcutPath
    if ($metadata.IconIndex -ne 0 -or $metadata.ShowCommand -ne 1 -or $metadata.TrailingBytes -ne 0) { throw 'Incorrect Shell Link metadata.' }
    if ($successfulShortcut) {
        if ((Split-Path $link.TargetPath -Leaf) -ne (Split-Path $successfulShortcut.TargetPath -Leaf) -or
            $metadata.IconIndex -ne $successfulShortcut.IconIndex -or $metadata.ShowCommand -ne $successfulShortcut.ShowCommand -or
            $metadata.HotKey -ne $successfulShortcut.HotKey -or $metadata.LinkFlags -ne $successfulShortcut.LinkFlags) {
            throw 'Shortcut Shell Link properties differ from successful BALD baseline.'
        }
        if (-not $referenceLink) {
            $baselineIconPath = $successfulShortcut.IconLocation -replace ',\s*-?\d+$', ''
            if ([IO.Path]::GetRelativePath($installPath, ($link.IconLocation -replace ',\s*-?\d+$', '')) -ne
                [IO.Path]::GetRelativePath($successfulShortcut.WorkingDirectory, $baselineIconPath)) {
                throw 'Shortcut relative icon location differs from successful BALD baseline.'
            }
        }
    }
    $metadata | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $work $(if ($referenceLink) { 'wscript-link.json' } else { 'installer-link.json' }))
    $registration = Get-ItemProperty -LiteralPath $uninstallKey
    if ($registration.DisplayName -ne $product -or $registration.DisplayVersion -ne $expectedVersion -or $registration.InstallLocation.Trim('"') -ne $installPath) { throw 'Uninstall registration incorrect.' }
    if (-not (Test-Path -LiteralPath (Join-Path $installPath 'uninstall.exe'))) { throw 'Uninstaller missing.' }
}
function Link-State([string]$path = $shortcutPath) {
    @((Get-FileHash -LiteralPath $path).Hash, (Get-Item -LiteralPath $path).LastWriteTimeUtc.Ticks, (Get-Item -LiteralPath $path).Attributes.ToString(), (Get-Acl -LiteralPath $path).Sddl) -join '|'
}
function Uninstall-TestApp {
    $uninstaller = Join-Path $installPath 'uninstall.exe'
    if (Test-Path -LiteralPath $uninstaller) {
        $process = Start-Process $uninstaller -ArgumentList "/S _?=$installPath" -WindowStyle Hidden -PassThru
        if (-not $process.WaitForExit(60000)) { $process.Kill(); throw 'Uninstaller timed out.' }
        if ($process.ExitCode -ne 0) { throw "Uninstaller failed: $($process.ExitCode)" }
    }
}
function Assert-Uninstalled {
    foreach ($path in @((Join-Path $installPath 'bald.exe'), (Join-Path $installPath 'icons\icon.ico'), $shortcutPath, $desktopPath, $uninstallKey)) {
        if (Test-Path -LiteralPath $path) { throw "Uninstall left behind: $path" }
    }
}
try {
    New-Item -ItemType Directory -Path (Split-Path $settingsPath) | Out-Null
    Set-Content -LiteralPath $settingsPath -Value '{"preserve":true}'
    $settingsHash = (Get-FileHash -LiteralPath $settingsPath).Hash
    Run-Installer '/S'; Assert-Installed
    $installerIcon = [InstallerIconProbe]::ShellIconHash($shortcutPath)
    $reference = Join-Path $work 'WScript-reference.lnk'
    $link = (New-Object -ComObject WScript.Shell).CreateShortcut($reference)
    $link.TargetPath = Join-Path $installPath 'bald.exe'
    $link.WorkingDirectory = $installPath
    $link.IconLocation = "$(Join-Path $installPath 'bald.exe'),0"
    $link.Save()
    if ([InstallerIconProbe]::ShellIconHash($reference) -ne $installerIcon) { throw 'Installer Shell icon differs from successful WScript recipe.' }
    if ($successfulShortcut -and [InstallerIconProbe]::ShellIconHash($realShortcut) -ne $installerIcon) { throw 'Installer Shell icon differs from successful BALD shortcut.' }
    Get-ShortcutMetadata $reference | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $work 'reference-link.json')
    Write-Output 'PASS: installed ICO matches WScript EXE icon and successful BALD shortcut through Shell rendering.'
    Write-Output 'PASS: fresh install, standard shortcut metadata, icon and uninstall entry.'
    $linkHash = Link-State
    $desktopState = Link-State $desktopPath
    New-ItemProperty -LiteralPath $runKey -Name $product -Value 'verification-only' -PropertyType String -Force | Out-Null
    Run-Installer '/S /UPDATE'; Assert-Installed
    if ((Get-ItemPropertyValue -LiteralPath $runKey -Name $product) -ne 'verification-only') { throw 'Update changed startup preference.' }
    if ((Link-State) -ne $linkHash -or (Link-State $desktopPath) -ne $desktopState) { throw 'Update rewrote a working shortcut.' }
    Write-Output 'PASS: update preserves working shortcut bytes and app identity.'
    Run-Installer '/S'; Assert-Installed
    if ((Link-State) -ne $linkHash -or (Link-State $desktopPath) -ne $desktopState) { throw 'Reinstall rewrote a working shortcut.' }
    Write-Output 'PASS: reinstall preserves shortcut bytes, timestamps, attributes and ACL.'
    # Only replace the isolated test link, never the real BALD shortcut.
    Copy-Item -LiteralPath $reference -Destination $shortcutPath -Force
    Assert-Installed $config.version $true
    $linkHash = Link-State
    Run-Installer '/S /UPDATE'; Assert-Installed $config.version $true
    Run-Installer '/S'; Assert-Installed $config.version $true
    if ((Link-State) -ne $linkHash) { throw 'Installer rewrote a user-created shortcut without explicit AppUserModelID.' }
    if ([InstallerIconProbe]::ShellIconHash($shortcutPath) -ne $installerIcon) { throw 'AppUserModelID changed Shell icon rendering.' }
    Write-Output 'PASS: WScript shortcut without explicit AppUserModelID survives update and reinstall; both identities render the same Shell icon.'
    $customLink = (New-Object -ComObject WScript.Shell).CreateShortcut($shortcutPath)
    $customLink.IconLocation = "$(Join-Path $installPath 'icons\icon.ico'),0"
    $customLink.Arguments = '--verification-only'
    $customLink.Description = 'User customization fixture'
    $customLink.Save()
    $linkHash = Link-State
    Run-Installer '/S /UPDATE'
    Run-Installer '/S'
    if ((Link-State) -ne $linkHash) { throw 'Installer changed custom shortcut icon, arguments or description.' }
    if ([InstallerIconProbe]::ShellIconHash($shortcutPath) -ne $installerIcon) { throw 'External ICO differs from embedded Shell icon.' }
    Write-Output 'PASS: existing external ICO, arguments and description are preserved.'
    Remove-Item -LiteralPath $shortcutPath
    Run-Installer '/S /UPDATE'; Assert-Installed
    Write-Output 'PASS: update restores a missing shortcut with the same app identity and valid icon.'
    Uninstall-TestApp; Assert-Uninstalled
    if (Get-ItemProperty -LiteralPath $runKey -Name $product -ErrorAction SilentlyContinue) { throw 'Uninstall retained startup entry.' }
    Write-Output 'PASS: uninstall removes EXE, icon, shortcuts and uninstall entry.'
    Run-Installer '/S'; Assert-Installed
    Uninstall-TestApp; Assert-Uninstalled
    Write-Output 'PASS: reinstall and subsequent uninstall.'
    Run-Installer '/S' 'setup-old.exe'; Assert-Installed '0.0.0'
    $linkHash = Link-State
    Run-Installer '/P' 'setup-maintenance.exe'; Assert-Installed
    if ((Link-State) -ne $linkHash) { throw 'Maintenance upgrade rewrote shortcut metadata.' }
    Write-Output 'PASS: maintenance upgrade runs the old uninstaller with /UPDATE and preserves shortcuts.'
    Run-Installer '/S /UPDATE'; Assert-Installed
    if ((Link-State) -ne $linkHash) { throw 'Version upgrade rewrote working shortcut metadata.' }
    Write-Output 'PASS: older-version fixture upgrades to the release version with stable path, identity and icon.'
    # Exercise the old-uninstaller /UPDATE contract used by interactive upgrades.
    $linkHash = Link-State
    $process = Start-Process (Join-Path $installPath 'uninstall.exe') -ArgumentList "/S /UPDATE _?=$installPath" -WindowStyle Hidden -PassThru
    if (-not $process.WaitForExit(60000)) { $process.Kill(); throw 'Upgrade uninstaller timed out.' }
    if ($process.ExitCode -ne 0 -or (Link-State) -ne $linkHash) { throw 'Upgrade uninstall removed or rewrote shortcut.' }
    Run-Installer '/S /UPDATE'; Assert-Installed
    if ((Link-State) -ne $linkHash) { throw 'Upgrade reinstall rewrote shortcut.' }
    Write-Output 'PASS: upgrade uninstall/reinstall retains shortcut metadata.'
    Uninstall-TestApp; Assert-Uninstalled
    Run-Installer '/S /NS'
    if (Test-Path -LiteralPath $shortcutPath) { throw 'Installer ignored the no-shortcut option.' }
    Run-Installer '/S /UPDATE'
    if (Test-Path -LiteralPath $shortcutPath) { throw 'Update ignored the saved no-shortcut preference.' }
    Uninstall-TestApp; Assert-Uninstalled
    Write-Output 'PASS: explicit no-shortcut installation is respected.'
    if ((Get-FileHash -LiteralPath $settingsPath).Hash -ne $settingsHash) { throw 'Installer changed user settings.' }
    Write-Output 'PASS: isolated user settings survive install, update and uninstall.'
} finally {
    Uninstall-TestApp
    # NSIS _?= keeps the running uninstaller at its original path. Remove only
    # explicitly named test files/directories after checking their resolved scope.
    if ([IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($installPath)) -ne $env:LOCALAPPDATA) { throw 'Unexpected cleanup path.' }
    if (Test-Path -LiteralPath (Join-Path $installPath 'uninstall.exe')) { Remove-Item -LiteralPath (Join-Path $installPath 'uninstall.exe') }
    if (Test-Path -LiteralPath $installPath) { Remove-Item -LiteralPath $installPath }
    if (Test-Path -LiteralPath $manufacturerKey) { Remove-Item -LiteralPath "$manufacturerKey\$product"; Remove-Item -LiteralPath $manufacturerKey }
    Remove-ItemProperty -LiteralPath $runKey -Name $product -ErrorAction SilentlyContinue
    if ([IO.Path]::GetDirectoryName([IO.Path]::GetFullPath((Split-Path $settingsPath))) -ne $env:APPDATA) { throw 'Unexpected settings cleanup path.' }
    Remove-Item -LiteralPath $settingsPath
    Remove-Item -LiteralPath (Split-Path $settingsPath)
    foreach ($path in $protectedHashes.Keys) {
        if ((Get-FileHash -LiteralPath $path).Hash -ne $protectedHashes[$path]) { throw "User file changed: $path" }
    }
    foreach ($path in $protectedState.Keys) {
        $state = if (Test-Path -LiteralPath $path) {
            @((Get-Item -LiteralPath $path).LastWriteTimeUtc.Ticks, (Get-Item -LiteralPath $path).Attributes.ToString(), (Get-Acl -LiteralPath $path).Sddl) -join '|'
        } else { 'absent' }
        if ($state -ne $protectedState[$path]) { throw "User file metadata changed: $path" }
    }
}
Write-Output 'PASS: existing BALD shortcut, installed files and user settings preserved. Actual BALD search/icon/launch UI remains a separate manual check.'
