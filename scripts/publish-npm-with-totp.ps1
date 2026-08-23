[CmdletBinding()]
param(
  [switch]$Publish,
  [switch]$Build,
  [string]$SecretFile,
  [string]$SecretEnvironmentVariable = "NPM_TOTP_SECRET",
  [string]$Registry = "https://registry.npmjs.org"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$packagePath = Join-Path $repoRoot "package.json"

function Get-SecretText {
  if ($SecretFile) {
    $resolved = (Resolve-Path -LiteralPath $SecretFile -ErrorAction Stop).Path
    return (Get-Content -LiteralPath $resolved -Raw).Trim()
  }

  $fromEnvironment = [Environment]::GetEnvironmentVariable(
    $SecretEnvironmentVariable,
    "Process"
  )
  if ($fromEnvironment) {
    return $fromEnvironment.Trim()
  }

  $secure = Read-Host "npm TOTP secret (local input only)" -AsSecureString
  $pointer = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure)
  try {
    return [Runtime.InteropServices.Marshal]::PtrToStringBSTR($pointer).Trim()
  } finally {
    [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($pointer)
  }
}

function ConvertFrom-Base32([string]$InputText) {
  $alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567"
  $buffer = 0
  $bits = 0
  $bytes = [Collections.Generic.List[byte]]::new()

  foreach ($character in $InputText.ToUpperInvariant().ToCharArray()) {
    if ($character -eq "=") { break }
    $index = $alphabet.IndexOf($character)
    if ($index -lt 0) {
      throw "TOTP secret is not valid Base32."
    }

    $buffer = ($buffer -shl 5) -bor $index
    $bits += 5
    if ($bits -ge 8) {
      $bits -= 8
      $bytes.Add([byte](($buffer -shr $bits) -band 255))
      if ($bits -eq 0) {
        $buffer = 0
      } else {
        $buffer = $buffer -band ((1 -shl $bits) - 1)
      }
    }
  }

  return $bytes.ToArray()
}

function Get-Totp([string]$Secret) {
  $normalized = $Secret.Trim() -replace "\s", ""
  if ($normalized -match "(?i)(?:^|[?&])secret=([^&]+)") {
    $normalized = [Uri]::UnescapeDataString($matches[1])
  }
  if (-not $normalized) {
    throw "TOTP secret is empty."
  }

  $key = ConvertFrom-Base32 $normalized
  if ($key.Length -eq 0) {
    throw "TOTP secret decoded to an empty key."
  }

  $counterValue = [int64][math]::Floor(
    ([DateTimeOffset]::UtcNow - [DateTimeOffset]::UnixEpoch).TotalSeconds / 30
  )
  $counter = [BitConverter]::GetBytes($counterValue)
  if ([BitConverter]::IsLittleEndian) {
    [Array]::Reverse($counter)
  }

  $hmac = [Security.Cryptography.HMACSHA1]::new($key)
  try {
    $hash = $hmac.ComputeHash($counter)
  } finally {
    $hmac.Dispose()
  }

  $offset = $hash[19] -band 15
  $binary =
    (($hash[$offset] -band 127) -shl 24) -bor
    ($hash[$offset + 1] -shl 16) -bor
    ($hash[$offset + 2] -shl 8) -bor
    $hash[$offset + 3]

  [pscustomobject]@{
    Code = ($binary % 1000000).ToString("D6")
    SecondsRemaining = 30 - ([DateTimeOffset]::UtcNow.ToUnixTimeSeconds() % 30)
  }
}

if (-not (Test-Path -LiteralPath $packagePath -PathType Leaf)) {
  throw "package.json was not found. Run this script from the public relay checkout."
}

$package = Get-Content -LiteralPath $packagePath -Raw | ConvertFrom-Json
if ($package.name -ne "qq-to-codex-app-server-relay") {
  throw "Unexpected package name: $($package.name)"
}
if ($package.private -eq $true) {
  throw "This checkout is private. Run the script from qq-to-codex-app-server-relay-public."
}
if ($package.version -ne "0.2.1") {
  throw "Expected release 0.2.1, found $($package.version)."
}

if ($Build) {
  Push-Location $repoRoot
  try {
    & pnpm.cmd build
    if ($LASTEXITCODE -ne 0) {
      throw "pnpm build failed with exit code $LASTEXITCODE."
    }
  } finally {
    Pop-Location
  }
}

$requiredFiles = @(
  "bin/qq-to-codex-app-server-relay.mjs",
  "apps/qq-adapter/dist/main.js",
  "target/release/relay-core.exe"
)
foreach ($relativePath in $requiredFiles) {
  $requiredPath = Join-Path $repoRoot $relativePath
  if (-not (Test-Path -LiteralPath $requiredPath -PathType Leaf)) {
    throw "Missing build artifact: $relativePath. Re-run with -Build."
  }
}

$publishedVersion = & npm.cmd view "$($package.name)@$($package.version)" version `
  --registry=$Registry 2>$null
if ($LASTEXITCODE -eq 0 -and $publishedVersion) {
  throw "$($package.name)@$($package.version) already exists on $Registry."
}

$totp = Get-Totp (Get-SecretText)
if (-not $Publish) {
  Write-Host "Current OTP: $($totp.Code) (about $($totp.SecondsRemaining)s remaining)"
  Write-Host "Re-run with -Publish to publish without copying the OTP."
  exit 0
}

Write-Host "Publishing $($package.name)@$($package.version) with an OTP valid for about $($totp.SecondsRemaining)s..."
& npm.cmd publish --ignore-scripts --registry=$Registry --otp=$($totp.Code)
if ($LASTEXITCODE -ne 0) {
  throw "npm publish failed with exit code $LASTEXITCODE. Generate a fresh OTP and retry."
}

Write-Host "Published. Verify with: npm view $($package.name) version --registry=$Registry"
