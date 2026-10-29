# install.ps1 - install eco on Windows from a GitHub release.
#
#   irm https://raw.githubusercontent.com/This-Is-NPC/eco/master/install.ps1 | iex
#
# It downloads the release's Windows installer (an MSI), checks it against the
# release's SHA256SUMS and runs it with Windows Installer, which asks for
# administrator rights and puts eco in C:\Program Files\eco for every user.
# An older per-user install in %LOCALAPPDATA%\Programs\eco (eco 0.9 and
# earlier) is removed first, with its PATH entry and Start-menu shortcut. A
# daemon that was running is started again. Sessions, config and models stay.
#
# Environment:
#   VERSION        release to install, e.g. 0.3.0 or v0.3.0 (default: the latest)
#   REPO           GitHub repository (default: This-Is-NPC/eco)
#   BASE_URL       where the release files are, instead of GitHub's download URL
#   DRY_RUN=1      download and verify, or name what would be removed, and stop
#   ECO_UNINSTALL  keep: take eco off, keep sessions, config and models;
#                  everything: take eco off and delete them too
& {
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

function Say([string] $message) { Write-Host "eco: $message" }

$repo = if ($env:REPO) { $env:REPO } else { 'This-Is-NPC/eco' }
$dryRun = $env:DRY_RUN -eq '1'
$eco = Join-Path $env:ProgramFiles 'eco'
# The per-user install of eco 0.9 and earlier.
$old = Join-Path $env:LOCALAPPDATA 'Programs\eco'
$oldBin = Join-Path $old 'bin'

# The user's PATH as stored, %...% entries unexpanded, and its registry type.
function Get-UserPath {
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment')
    try {
        $value = $key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
        $kind = if ($null -eq $key.GetValue('Path')) { [Microsoft.Win32.RegistryValueKind]::ExpandString } else { $key.GetValueKind('Path') }
        return @($value, $kind)
    } finally { $key.Close() }
}

# Writes the user's PATH with its type kept, then sends WM_SETTINGCHANGE to
# every top-level window, so Explorer gives it to the programs it starts.
function Set-UserPath([string] $value, $kind) {
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true)
    try { $key.SetValue('Path', $value, $kind) } finally { $key.Close() }
    if (-not ('EcoInstall.Native' -as [type])) {
        Add-Type -Namespace EcoInstall -Name Native -MemberDefinition @'
[DllImport("user32.dll", CharSet = CharSet.Unicode)]
public static extern IntPtr SendMessageTimeout(IntPtr hWnd, uint msg, UIntPtr wParam, string lParam, uint flags, uint timeout, out UIntPtr result);
'@
    }
    $result = [UIntPtr]::Zero
    [void][EcoInstall.Native]::SendMessageTimeout([IntPtr]0xFFFF, 0x1A, [UIntPtr]::Zero, 'Environment', 2, 5000, [ref]$result)
}

function Test-OldBin([string] $entry) {
    [Environment]::ExpandEnvironmentVariables($entry).TrimEnd('\') -eq $oldBin.TrimEnd('\')
}

# Deletes a folder, waiting up to 5 seconds for processes that have just
# exited to let go of its files.
function Remove-Folder([string] $path) {
    for ($i = 0; $i -lt 50; $i++) {
        if (-not (Test-Path -LiteralPath $path)) { return }
        try { Remove-Item -LiteralPath $path -Recurse -Force; return } catch { Start-Sleep -Milliseconds 100 }
    }
    Remove-Item -LiteralPath $path -Recurse -Force
}

# Stops the daemon of the eco installed in `folder` and the windows run from
# it. Returns $null when no daemon ran, else whether its window was open.
function Stop-Eco([string] $folder) {
    $exe = Join-Path $folder 'bin\eco.exe'
    $window = $null
    if (Test-Path -LiteralPath $exe) {
        & $exe status | Out-Null
        if ($LASTEXITCODE -eq 0) {
            & $exe status --window-open | Out-Null
            $window = $LASTEXITCODE -eq 0
            & $exe stop | Out-Host
            if ($LASTEXITCODE -ne 0) { throw 'eco: could not stop the running eco; nothing was changed.' }
        } elseif ($LASTEXITCODE -ne 3) {
            throw 'eco: cannot tell whether the eco daemon is running; nothing was changed.'
        }
    }
    Get-Process -Name eco-window -ErrorAction SilentlyContinue |
        Where-Object { $_.Path -like "$folder\*" } | Stop-Process -Force
    return $window
}

# Removes the per-user install of eco 0.9 and earlier: its folder, PATH entry,
# Start-menu shortcut, login script and the app id key. Returns whether its
# daemon ran, as Stop-Eco does.
function Remove-PerUserInstall {
    $programs = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs'
    $files = @((Join-Path $programs 'Startup\eco.cmd'), (Join-Path $programs 'eco.lnk'))
    $appId = 'HKCU:\Software\Classes\AppUserModelId\This-Is-NPC.eco'
    $value, $kind = Get-UserPath
    $entries = @($value -split ';')
    $kept = @($entries | Where-Object { -not ($_ -and (Test-OldBin $_)) })
    if (-not (Test-Path -LiteralPath $old)) { return $null }
    if ($dryRun) {
        foreach ($path in @($old) + $files + $appId) { if (Test-Path -LiteralPath $path) { Say "would remove $path" } }
        if ($kept.Count -ne $entries.Count) { Say "would remove $oldBin from your PATH" }
        return $null
    }
    $window = Stop-Eco $old
    Remove-Folder $old
    Say "removed $old"
    foreach ($path in $files) {
        if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path -Force; Say "removed $path" }
    }
    if (Test-Path -LiteralPath $appId) { Remove-Item -LiteralPath $appId -Recurse -Force }
    if ($kept.Count -ne $entries.Count) {
        Set-UserPath ($kept -join ';') $kind
        Say "removed $oldBin from your PATH"
    }
    return $window
}

# Every registration of eco with Windows Installer, found by its name and
# publisher: more than one when an eco installed twice at the same version
# by an older package left both.
function Get-Product {
    @(Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*' -ErrorAction SilentlyContinue |
        Where-Object { $_.DisplayName -eq 'eco' -and $_.Publisher -eq 'This-Is-NPC' -and $_.WindowsInstaller -eq 1 })
}

# Downloads `url` to `path` with Windows' own curl, which draws its progress,
# tries a dropped connection again and gives up on one that sends nothing for
# a minute, so a slow download shows it moves and a dead one ends.
function Get-Download([string] $url, [string] $path, [switch] $quiet) {
    [string[]] $show = if ($quiet) { '--silent', '--show-error' } else { '--progress-bar' }
    & (Join-Path $env:SystemRoot 'System32\curl.exe') --fail --location --retry 3 `
        --connect-timeout 30 --speed-limit 1 --speed-time 60 @show --output $path $url
    if ($LASTEXITCODE -ne 0) { throw "eco: could not download $url (curl exited with status $LASTEXITCODE)." }
}

# Runs msiexec with `arguments` and waits. Windows Installer asks for
# administrator rights itself; 3010 asks for a restart.
function Invoke-Msiexec([string[]] $arguments) {
    $code = (Start-Process msiexec -ArgumentList ($arguments + '/qb', '/norestart') -Wait -PassThru).ExitCode
    if ($code -notin 0, 3010) { throw "eco: Windows Installer exited with status $code; see the log it wrote, or run the installer again." }
    if ($code -eq 3010) { Say 'restart Windows to finish' }
}

function Uninstall-Eco([bool] $everything) {
    $data = @((Join-Path $env:APPDATA 'eco'), (Join-Path $env:LOCALAPPDATA 'eco'))
    $products = Get-Product
    [void](Remove-PerUserInstall)
    if ($dryRun) {
        if ($products) { Say "would remove $eco with Windows Installer" }
        if ($everything) { foreach ($path in $data) { if (Test-Path -LiteralPath $path) { Say "would remove $path" } } }
        return
    }
    if ($products) {
        [void](Stop-Eco $eco)
        foreach ($product in $products) { Invoke-Msiexec @('/x', $product.PSChildName) }
        Say "removed $eco"
    }
    foreach ($path in $data) {
        if (-not (Test-Path -LiteralPath $path)) { continue }
        if ($everything) { Remove-Folder $path; Say "removed $path" } else { Say "kept $path" }
    }
}

function Install-Eco {
    if (-not [Environment]::Is64BitOperatingSystem) {
        throw 'eco: there is only an x86_64 build, and this Windows is 32-bit.'
    }
    $base = $env:BASE_URL
    if (-not $base) {
        if ($env:VERSION) {
            $tag = 'v' + $env:VERSION.TrimStart('v')
        } else {
            try {
                $tag = (Invoke-RestMethod -UseBasicParsing "https://api.github.com/repos/$repo/releases/latest").tag_name
            } catch { throw "eco: found no published release of $repo." }
        }
        $base = "https://github.com/$repo/releases/download/$tag"
    }
    $base = $base.TrimEnd('/')

    $tmp = Join-Path ([IO.Path]::GetTempPath()) ('eco-' + [Guid]::NewGuid())
    [void](New-Item -ItemType Directory -Path $tmp)
    try {
        $sums = Join-Path $tmp 'SHA256SUMS'
        Get-Download "$base/SHA256SUMS" $sums -quiet
        $line = Get-Content -LiteralPath $sums |
            Where-Object { $_ -match '^([0-9a-fA-F]{64}) +\*?(eco-[0-9][^ ]*-windows-x86_64\.msi)$' } |
            Select-Object -First 1
        if (-not $line) { throw "eco: the release's SHA256SUMS names no Windows installer." }
        [void]($line -match '^([0-9a-fA-F]{64}) +\*?(.+)$')
        $expected, $name = $Matches[1], $Matches[2]
        $msi = Join-Path $tmp $name

        Say "downloading $name"
        Get-Download "$base/$name" $msi
        if ((Get-FileHash -LiteralPath $msi -Algorithm SHA256).Hash -ne $expected) {
            throw "eco: $name does not match the release's SHA256SUMS; nothing was installed."
        }
        if ($dryRun) {
            Say "$name matches SHA256SUMS; would install it in $eco"
            [void](Remove-PerUserInstall)
            return
        }

        $window = Remove-PerUserInstall
        if ($null -eq $window) { $window = Stop-Eco $eco }
        Invoke-Msiexec @('/i', "`"$msi`"")
        Say "installed eco in $eco"
    } finally {
        Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
    }

    $exe = Join-Path $eco 'bin\eco.exe'
    if ($null -ne $window) {
        & $exe start --headless | Out-Host
        if ($window) { & $exe start | Out-Host }
    }
    Say 'installed. Next: open eco from the Start menu, or run eco start in a new terminal'
    Say 'take it off again with: $env:ECO_UNINSTALL = "keep"; irm https://raw.githubusercontent.com/This-Is-NPC/eco/master/install.ps1 | iex'
}

try {
    if (-not $env:ECO_UNINSTALL) { Install-Eco }
    elseif ($env:ECO_UNINSTALL -eq 'keep') { Uninstall-Eco $false }
    elseif ($env:ECO_UNINSTALL -eq 'everything') { Uninstall-Eco $true }
    else { throw "eco: ECO_UNINSTALL is keep or everything, not $($env:ECO_UNINSTALL)." }
} catch {
    Write-Host $_.Exception.Message -ForegroundColor Red
    # Run as a file, fail with status 1; through iex, leave the session open.
    if ($PSCommandPath) { exit 1 }
}
}
