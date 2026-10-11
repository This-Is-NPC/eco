# install.ps1 - install eco on Windows from a GitHub release, for your account.
#
#   irm https://raw.githubusercontent.com/This-Is-NPC/eco/master/install.ps1 | iex
#
# It downloads the release's Windows zip, checks it against the release's
# SHA256SUMS, stops a running eco, puts the zip's eco folder in
# %LOCALAPPDATA%\Programs\eco, adds its bin to your PATH and runs `eco setup`.
# A daemon that was running is started again. Sessions, config and models stay.
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
$eco = Join-Path $env:LOCALAPPDATA 'Programs\eco'
$bin = Join-Path $eco 'bin'
$exe = Join-Path $bin 'eco.exe'

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

function Test-SameDirectory([string] $entry) {
    [Environment]::ExpandEnvironmentVariables($entry).TrimEnd('\') -eq $bin.TrimEnd('\')
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

# Stops the installed daemon and the windows run from the install. Returns
# $null when no daemon ran, else whether its window was open.
function Stop-Eco {
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
        Where-Object { $_.Path -like "$eco\*" } | Stop-Process -Force
    return $window
}

function Uninstall-Eco([bool] $everything) {
    $data = @((Join-Path $env:APPDATA 'eco'), (Join-Path $env:LOCALAPPDATA 'eco'))
    $programs = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs'
    $folders = @($eco) + $(if ($everything) { $data } else { @() })
    $files = @((Join-Path $programs 'Startup\eco.cmd'), (Join-Path $programs 'eco.lnk'))
    $value, $kind = Get-UserPath
    $entries = @($value -split ';')
    $kept = @($entries | Where-Object { -not ($_ -and (Test-SameDirectory $_)) })
    if ($dryRun) {
        foreach ($path in $folders + $files) { if (Test-Path -LiteralPath $path) { Say "would remove $path" } }
        if ($kept.Count -ne $entries.Count) { Say "would remove $bin from your PATH" }
        return
    }
    [void](Stop-Eco)
    foreach ($path in $folders) {
        if (Test-Path -LiteralPath $path) { Remove-Folder $path; Say "removed $path" }
    }
    foreach ($path in $files) {
        if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path -Force; Say "removed $path" }
    }
    if ($kept.Count -ne $entries.Count) {
        Set-UserPath ($kept -join ';') $kind
        Say "removed $bin from your PATH"
    }
    if (-not $everything) {
        foreach ($path in $data) { if (Test-Path -LiteralPath $path) { Say "kept $path" } }
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
        [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
        $web = New-Object Net.WebClient
        $sums = Join-Path $tmp 'SHA256SUMS'
        try { $web.DownloadFile("$base/SHA256SUMS", $sums) } catch { throw "eco: could not download $base/SHA256SUMS." }
        $line = Get-Content -LiteralPath $sums |
            Where-Object { $_ -match '^([0-9a-fA-F]{64}) +\*?(eco-[0-9][^ ]*-windows-x86_64\.zip)$' } |
            Select-Object -First 1
        if (-not $line) { throw "eco: the release's SHA256SUMS names no Windows zip." }
        [void]($line -match '^([0-9a-fA-F]{64}) +\*?(.+)$')
        $expected, $name = $Matches[1], $Matches[2]
        $zip = Join-Path $tmp $name

        Say "downloading $name"
        try { $web.DownloadFile("$base/$name", $zip) } catch { throw "eco: could not download $base/$name." }
        if ((Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash -ne $expected) {
            throw "eco: $name does not match the release's SHA256SUMS; nothing was installed."
        }
        if ($dryRun) {
            Say "$name matches SHA256SUMS; would install it in $eco, add $bin to your PATH and run eco setup"
            return
        }

        $window = Stop-Eco
        Remove-Folder $eco
        Add-Type -AssemblyName System.IO.Compression.FileSystem
        [IO.Compression.ZipFile]::ExtractToDirectory($zip, (Split-Path $eco))
        Say "installed eco in $eco"
    } finally {
        Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
    }

    $value, $kind = Get-UserPath
    if (@($value -split ';' | Where-Object { $_ -and (Test-SameDirectory $_) }).Count -gt 0) {
        Say "$bin is on your PATH already"
    } else {
        Set-UserPath $(if ($value.Trim(';')) { $value.TrimEnd(';') + ";$bin" } else { $bin }) $kind
        Say "added $bin to your PATH; a new terminal finds eco"
    }
    if (-not (@($env:Path -split ';') | Where-Object { $_ -and (Test-SameDirectory $_) })) {
        $env:Path = $env:Path.TrimEnd(';') + ";$bin"
    }

    & $exe setup | Out-Host
    if ($LASTEXITCODE -ne 0) { throw 'eco: eco setup failed; run it again to see why.' }
    if ($null -ne $window) {
        & $exe start --headless | Out-Host
        if ($window) { & $exe start | Out-Host }
    }
    Say 'installed. Next: open eco from the Start menu, or run eco start'
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
