# Install chromium-bridge from GitHub Releases.
# Usage: irm https://raw.githubusercontent.com/btakita/chromium-bridge/main/install.ps1 | iex
[CmdletBinding()]
param(
    [string] $Version = 'latest',
    [string] $InstallDir = (Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'Programs\chromium-bridge'),
    [string] $ArchivePath,
    [switch] $NoPathUpdate
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$repository = 'btakita/chromium-bridge'
$temporaryRoot = Join-Path ([IO.Path]::GetTempPath()) ("chromium-bridge-{0}" -f [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $temporaryRoot | Out-Null

try {
    if ($ArchivePath) {
        $archive = (Resolve-Path -LiteralPath $ArchivePath).Path
        $tag = if ($Version -eq 'latest') { 'local package' } elseif ($Version.StartsWith('v')) { $Version } else { "v$Version" }
    }
    else {
        $targetArchitecture = switch ([Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()) {
            'X64' { 'x86_64' }
            'Arm64' { 'aarch64' }
            default { throw "Unsupported Windows architecture: $($_)" }
        }

        $releaseApi = if ($Version -eq 'latest') {
            "https://api.github.com/repos/$repository/releases/latest"
        }
        else {
            $tag = if ($Version.StartsWith('v')) { $Version } else { "v$Version" }
            $encodedTag = [Uri]::EscapeDataString($tag)
            "https://api.github.com/repos/$repository/releases/tags/$encodedTag"
        }

        $headers = @{
            Accept = 'application/vnd.github+json'
            'User-Agent' = 'chromium-bridge-installer'
            'X-GitHub-Api-Version' = '2022-11-28'
        }
        $release = Invoke-RestMethod -Uri $releaseApi -Headers $headers
        $tag = $release.tag_name
        $filename = "chromium-bridge-$tag-$targetArchitecture-pc-windows-msvc.zip"
        $assets = @($release.assets | Where-Object { $_.name -eq $filename })
        if ($assets.Count -ne 1) {
            throw "Release $tag does not contain the Windows package $filename"
        }

        $asset = $assets[0]
        $archive = Join-Path $temporaryRoot $filename
        Write-Host "Downloading chromium-bridge $tag for $targetArchitecture Windows..."
        Invoke-WebRequest -Uri $asset.browser_download_url -Headers $headers -UseBasicParsing -OutFile $archive

        $digestProperty = $asset.PSObject.Properties['digest']
        if ($null -eq $digestProperty -or $digestProperty.Value -notlike 'sha256:*') {
            throw "Release asset $filename does not provide a SHA-256 digest"
        }
        $expectedDigest = $digestProperty.Value.Substring(7)
        $actualDigest = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash
        if ($actualDigest -ine $expectedDigest) {
            throw "SHA-256 mismatch for $filename"
        }
    }

    $extractDir = Join-Path $temporaryRoot 'package'
    Expand-Archive -LiteralPath $archive -DestinationPath $extractDir -Force
    $sourceBinary = Join-Path $extractDir 'chromium-bridge.exe'
    if (-not (Test-Path -LiteralPath $sourceBinary -PathType Leaf)) {
        throw 'The Windows package does not contain chromium-bridge.exe'
    }

    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    $destination = Join-Path $InstallDir 'chromium-bridge.exe'
    Copy-Item -LiteralPath $sourceBinary -Destination $destination -Force

    if (-not $NoPathUpdate) {
        $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
        $installPath = $InstallDir.TrimEnd([IO.Path]::DirectorySeparatorChar)
        $pathEntries = @($userPath -split ';' | Where-Object { $_ })
        $alreadyOnPath = $pathEntries | Where-Object {
            $_.Trim().TrimEnd([IO.Path]::DirectorySeparatorChar) -ieq $installPath
        }

        if (-not $alreadyOnPath) {
            $newUserPath = if ($userPath) { "$userPath;$InstallDir" } else { $InstallDir }
            [Environment]::SetEnvironmentVariable('Path', $newUserPath, 'User')
        }
        if (-not (($env:Path -split ';') -icontains $InstallDir)) {
            $env:Path = "$InstallDir;$env:Path"
        }
    }

    Write-Host "Installed chromium-bridge $tag to $destination"
    if (-not $NoPathUpdate) {
        Write-Host 'Open a new terminal if chromium-bridge is not yet on PATH.'
    }
}
finally {
    Remove-Item -LiteralPath $temporaryRoot -Recurse -Force -ErrorAction SilentlyContinue
}
