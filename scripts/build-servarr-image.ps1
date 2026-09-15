[CmdletBinding()]
param(
    [ValidateSet("linux/amd64", "linux/arm64", "linux/arm/v7")]
    [string]$Platform = "linux/amd64",

    [string]$Image = "rqbit-servarr:test",

    [string]$Archive
)

$ErrorActionPreference = "Stop"
$repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$platformKey = $Platform.Replace("/", "-")

if (-not $Archive) {
    # Never reuse the default archive name: every invocation produces a separately
    # deployable image archive. UTC keeps names sortable across build machines.
    $buildVersion = [DateTime]::UtcNow.ToString("yyyyMMddTHHmmssfffZ")
    $Archive = Join-Path $repoRoot "target/rqbit-servarr-$platformKey-$buildVersion.tar"

    # A timestamp collision is unlikely, but do not overwrite a prior deployment
    # artifact if two runs begin in the same millisecond.
    $suffix = 1
    while (Test-Path -LiteralPath $Archive) {
        $Archive = Join-Path $repoRoot "target/rqbit-servarr-$platformKey-$buildVersion-$suffix.tar"
        $suffix++
    }
} elseif (-not [IO.Path]::IsPathRooted($Archive)) {
    $Archive = Join-Path $repoRoot $Archive
}
$Archive = [IO.Path]::GetFullPath($Archive)

function Assert-LastExitCode {
    param([string]$Step)

    if ($LASTEXITCODE -ne 0) {
        throw "$Step failed with exit code $LASTEXITCODE"
    }
}

function Write-Step {
    param([string]$Message)

    Write-Host "`n==> $Message" -ForegroundColor Cyan
}

$npm = if ($env:OS -eq "Windows_NT") { "npm.cmd" } else { "npm" }
Get-Command docker -ErrorAction Stop | Out-Null
Get-Command $npm -ErrorAction Stop | Out-Null

Push-Location $repoRoot
try {
    Write-Step "Checking Docker"
    & docker info --format "Docker Engine {{.ServerVersion}}"
    Assert-LastExitCode "Docker availability check"

    Write-Step "Installing the locked Web UI dependencies"
    & $npm ci --workspace crates/librqbit/webui
    Assert-LastExitCode "Web UI dependency installation"

    Write-Step "Building the embedded Web UI"
    & $npm run build --workspace crates/librqbit/webui
    Assert-LastExitCode "Web UI build"

    $requiredWebUiFiles = @(
        "crates/librqbit/webui/dist/index.html",
        "crates/librqbit/webui/dist/assets/index.js",
        "crates/librqbit/webui/dist/assets/index.css",
        "crates/librqbit/webui/dist/assets/logo.svg"
    )
    foreach ($file in $requiredWebUiFiles) {
        if (-not (Test-Path -LiteralPath $file -PathType Leaf)) {
            throw "Web UI build did not create $file"
        }
    }

    Write-Step "Creating an LF-only temporary cross-build Dockerfile"
    $temporaryDockerfile = Join-Path $repoRoot "target/Dockerfile.xx.lf"
    New-Item -ItemType Directory -Force (Split-Path $temporaryDockerfile) | Out-Null
    $dockerfile = [IO.File]::ReadAllText(
        (Join-Path $repoRoot "docker/Dockerfile.xx")
    ).Replace("`r`n", "`n").Replace("`r", "`n")
    [IO.File]::WriteAllText(
        $temporaryDockerfile,
        $dockerfile,
        [Text.UTF8Encoding]::new($false)
    )

    $crossOutput = "target/cross/$Platform"
    New-Item -ItemType Directory -Force $crossOutput | Out-Null

    Write-Step "Building the static rqbit binary for $Platform"
    & docker build `
        --platform $Platform `
        -f $temporaryDockerfile `
        --output "type=local,dest=$crossOutput" `
        .
    Assert-LastExitCode "Static Linux build"

    Write-Step "Building container image $Image"
    & docker build `
        --platform $Platform `
        -f docker/Dockerfile `
        -t $Image `
        target/cross
    Assert-LastExitCode "Container image build"

    Write-Step "Smoke-testing the image without network access"
    & docker run --rm --network none $Image --version
    Assert-LastExitCode "Container smoke test"

    Write-Step "Exporting $Image"
    New-Item -ItemType Directory -Force (Split-Path $Archive) | Out-Null
    & docker save --output $Archive $Image
    Assert-LastExitCode "Container image export"

    $archiveInfo = Get-Item -LiteralPath $Archive
    $checksum = (Get-FileHash -LiteralPath $Archive -Algorithm SHA256).Hash.ToLowerInvariant()

    Write-Host "`nBuild complete." -ForegroundColor Green
    Write-Host "Image:   $Image"
    Write-Host "Platform: $Platform"
    Write-Host "Archive: $($archiveInfo.FullName)"
    Write-Host "Size:    $([math]::Round($archiveInfo.Length / 1MB, 1)) MiB"
    Write-Host "SHA256:  $checksum"
    Write-Host "`nOn the Docker server, run:"
    Write-Host "  docker load --input `"$($archiveInfo.Name)`""
} finally {
    Pop-Location
}
