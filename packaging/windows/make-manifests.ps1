# 由 CI 在打标签时调用：根据已构建的安装包生成 winget / Scoop / Chocolatey 清单，
# 并把清单打包成 nupkg 与一个 zip 一起放进 dist/，随 GitHub Release 一起发布。
#
# 用法：packaging\windows\make-manifests.ps1 -Version 1.2.3 [-DistDir dist] [-Repository owner/repo]
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Version,
    [string]$DistDir = 'dist',
    [string]$Repository = $env:GITHUB_REPOSITORY
)

$ErrorActionPreference = 'Stop'

$manifestSrc = Join-Path $PSScriptRoot 'manifests'

$setup = Get-ChildItem -Path $DistDir -Filter "PacketSniffer-$Version-*-setup.exe" | Select-Object -First 1
if (-not $setup) { throw "找不到 NSIS 安装包：$DistDir\PacketSniffer-$Version-*-setup.exe" }

$portable = Get-ChildItem -Path $DistDir -Filter "packet-sniffer-$Version-windows-x86_64.zip" | Select-Object -First 1
if (-not $portable) { throw "找不到便携包：$DistDir\packet-sniffer-$Version-windows-x86_64.zip" }

$baseUrl = "https://github.com/$Repository/releases/download/v$Version"
$installerSha = (Get-FileHash -LiteralPath $setup.FullName -Algorithm SHA256).Hash
$portableSha = (Get-FileHash -LiteralPath $portable.FullName -Algorithm SHA256).Hash.ToLower()

$values = @{
    '__VERSION__'         = $Version
    '__PUBLISHER__'       = 'aafnnp'
    '__HOMEPAGE__'        = "https://github.com/$Repository"
    '__LICENSE_URL__'     = "https://github.com/$Repository/blob/master/LICENSE"
    '__INSTALLER_URL__'   = "$baseUrl/$($setup.Name)"
    '__INSTALLER_SHA256__' = $installerSha
    '__PORTABLE_URL__'    = "$baseUrl/$($portable.Name)"
    '__PORTABLE_SHA256__' = $portableSha
}

function Render-Template([string]$templatePath, [string]$destination) {
    $content = Get-Content -Raw -LiteralPath $templatePath
    foreach ($key in $values.Keys) {
        $content = $content.Replace($key, $values[$key])
    }
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $destination) | Out-Null
    Set-Content -LiteralPath $destination -Value $content -Encoding utf8 -NoNewline
}

$stage = Join-Path $env:RUNNER_TEMP 'manifests'
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }

Get-ChildItem -Path $manifestSrc -Recurse -File -Filter '*.template' | ForEach-Object {
    $relative = $_.FullName.Substring($manifestSrc.Length + 1) -replace '\.template$', ''
    Render-Template $_.FullName (Join-Path $stage $relative)
}

$archive = Join-Path $DistDir "packet-sniffer-$Version-manifests.zip"
Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $archive -Force

$nuspec = Join-Path $stage 'chocolatey\packet-sniffer.nuspec'
choco pack $nuspec --output-directory $DistDir
if ($LASTEXITCODE -ne 0) { throw "choco pack 失败：$LASTEXITCODE" }

Write-Host "已生成清单：$archive"
Get-ChildItem -Path $stage -Recurse -File | ForEach-Object {
    Write-Host "--- $($_.FullName.Substring($stage.Length + 1)) ---"
    Get-Content -LiteralPath $_.FullName | Write-Host
}
Get-ChildItem -Path $DistDir -Filter '*nupkg' | Format-Table Name, Length
