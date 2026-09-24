<#
.SYNOPSIS
    Signs files with a certificate from the environment. Needs no cargo.

.DESCRIPTION
    The signing jobs of CI and of the release workflow run this and nothing else, so the key
    never shares a machine with the build scripts of Mujina's dependencies (docs/signing.md).

    1. Writes the PFX (base64 in one environment variable, its password in another) to
       $env:RUNNER_TEMP under a random name.
    2. Signs each file with signtool from the newest Windows SDK: SHA-256, with an RFC 3161
       timestamp. If the first timestamp server fails, the file goes back to unsigned and is
       signed again with the second.
    3. Checks each signature (see Get-SignatureProblem).
    4. Deletes the PFX it wrote, whatever happened (a -PfxFile stays).

    The password goes to signtool only; this script never prints it. signtool itself leaves a
    copy of the private key, one per call, in the key store of the user it runs as
    (%APPDATA%\Microsoft\Crypto\Keys). A hosted runner is thrown away after the job; on a
    machine that is kept, sign from the certificate store instead.

.PARAMETER Path
    The files to sign; wildcards allowed. They must not be signed yet: a package cannot carry
    a second signature.

.PARAMETER PfxVariable
    The environment variable that holds the PFX, base64-encoded.

.PARAMETER PfxFile
    A PFX file to sign with instead, for a release-shaped build on a developer's own machine
    (cargo xtask dist). It is used where it is and never deleted; its password is still read
    from -PasswordVariable. signtool leaves a copy of its private key in your key store (see
    above): use a development certificate, never the release one.

.PARAMETER PasswordVariable
    The environment variable that holds the PFX's password.

.PARAMETER Subject
    If given, the certificate's subject must be exactly this. For a package it is the
    manifest's publisher (MSIX_PUBLISHER); signtool refuses a package whose publisher differs.

.PARAMETER Certificate
    If given, the public certificate is written to this file (.cer), for Mujina Setup and
    for testers.

.EXAMPLE
    pwsh packaging/sign.ps1 -Path target/package/*.msix -Subject 'CN=Mujina Dev' -Certificate Mujina-Dev.cer
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)] [string[]] $Path,
    [string] $PfxVariable = 'SIGNING_PFX_BASE64',
    [string] $PfxFile,
    [string] $PasswordVariable = 'SIGNING_PFX_PASSWORD',
    [string] $Subject,
    [string] $Certificate
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 3.0

# RFC 3161 servers, in the order they are tried. Plain HTTP is what both offer and is enough:
# the answer is itself signed. Sectigo asks for 15 s between requests; it is only the fallback.
$TimestampServers = @('http://timestamp.digicert.com', 'http://timestamp.sectigo.com')

# signtool.exe of the newest Windows SDK. The versions are compared as numbers: as text,
# 10.0.9999.0 would count as newer than 10.0.26100.0.
function Find-SignTool {
    $bin = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
    $newest = Get-ChildItem -LiteralPath $bin -Directory -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -match '^\d+(\.\d+){1,3}$' -and (Test-Path -LiteralPath (Join-Path $_.FullName 'x64\signtool.exe')) } |
        Sort-Object { [version] $_.Name } -Descending |
        Select-Object -First 1
    if (-not $newest) { throw "signtool.exe not found in any Windows SDK under $bin" }
    Join-Path $newest.FullName 'x64\signtool.exe'
}

# Why the signature of $File is not the one wanted, or $null if it is.
#
# `signtool verify /pa` cannot decide this: it fails for every self-signed certificate the
# machine does not trust, which on a runner is always. Get-AuthenticodeSignature reads the
# signer and the timestamp whatever its verdict, and reports a chain that ends in an untrusted
# root as UnknownError, while a damaged file is HashMismatch or NotSigned. So the signer must
# be exactly this certificate, a timestamp must be there, and the status must be Valid, or
# UnknownError for a self-signed certificate. (StatusMessage is localised; it is only shown.)
function Get-SignatureProblem([string] $File, $Expected) {
    $signature = Get-AuthenticodeSignature -LiteralPath $File
    if (-not $signature.SignerCertificate) { return "not signed ($($signature.Status))" }
    if ($signature.SignerCertificate.Thumbprint -ne $Expected.Thumbprint) {
        return "signed by $($signature.SignerCertificate.Subject), not by this certificate"
    }
    if (-not $signature.TimeStamperCertificate) { return 'no timestamp' }
    $selfSigned = $Expected.Subject -eq $Expected.Issuer
    if ($signature.Status -eq 'Valid' -or ($selfSigned -and $signature.Status -eq 'UnknownError')) {
        return $null
    }
    "$($signature.Status): $($signature.StatusMessage)"
}

$password = [Environment]::GetEnvironmentVariable($PasswordVariable)
if ($PfxFile) {
    $PfxFile = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($PfxFile)
    if (-not (Test-Path -LiteralPath $PfxFile -PathType Leaf)) { throw "$PfxFile not found" }
    $pfxBase64 = $null
    if (-not $password) { throw "Set $PasswordVariable to sign with $PfxFile." }
} else {
    $pfxBase64 = [Environment]::GetEnvironmentVariable($PfxVariable)
    if (-not $pfxBase64 -or -not $password) { throw "Set $PfxVariable and $PasswordVariable to sign." }
}

$files = foreach ($item in $Path) {
    $found = @(Get-Item -Path $item | Where-Object { -not $_.PSIsContainer })
    if ($found.Count -eq 0) { throw "Nothing to sign at $item" }
    $found | ForEach-Object { $_.FullName }
}
foreach ($file in $files) {
    if ((Get-AuthenticodeSignature -LiteralPath $file).SignerCertificate) { throw "$file is signed already" }
}

$temp = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { [IO.Path]::GetTempPath() }
# The developer's own file is used where it is; the secret is written to a file of this run's.
$pfx = if ($PfxFile) { $PfxFile } else { Join-Path $temp "$([guid]::NewGuid().ToString('N')).pfx" }
$signer = $null
try {
    if ($PfxFile) {
        $bytes = [IO.File]::ReadAllBytes($PfxFile)
    } else {
        $bytes = [Convert]::FromBase64String($pfxBase64)
        [IO.File]::WriteAllBytes($pfx, $bytes)
    }
    # Only the public part is needed here; EphemeralKeySet keeps the private key off the disk.
    $signer = [Security.Cryptography.X509Certificates.X509Certificate2]::new(
        $bytes, $password, [Security.Cryptography.X509Certificates.X509KeyStorageFlags]::EphemeralKeySet)
    Write-Host "Certificate: $($signer.Subject)"
    Write-Host "  thumbprint $($signer.Thumbprint), valid until $($signer.NotAfter.ToString('yyyy-MM-dd'))"
    if ($Subject -and $signer.Subject -ne $Subject) {
        throw "The certificate is $($signer.Subject), not $Subject."
    }
    if ($signer.NotAfter -lt (Get-Date)) { throw 'The certificate has expired.' }
    if ($signer.NotAfter -lt (Get-Date).AddDays(180)) {
        Write-Warning 'The certificate expires within half a year: rotate it (docs/signing.md).'
    }

    $signtool = Find-SignTool
    Write-Host "signtool: $signtool"
    foreach ($file in $files) {
        $unsigned = Join-Path $temp ([guid]::NewGuid().ToString('N'))
        Copy-Item -LiteralPath $file -Destination $unsigned
        try {
            $problem = 'not tried'
            foreach ($server in $TimestampServers) {
                & $signtool sign /fd SHA256 /tr $server /td SHA256 /f $pfx /p $password $file
                # 1 is a failure. 2 is "done with warnings", which includes a timestamp that
                # never came, so the signature itself decides.
                $problem = if ($LASTEXITCODE -eq 1) { 'signtool failed' } else { Get-SignatureProblem $file $signer }
                if (-not $problem) {
                    Write-Host "signed $file, timestamped by $server"
                    break
                }
                Write-Warning "$(Split-Path -Leaf $file) with ${server}: $problem"
                Copy-Item -LiteralPath $unsigned -Destination $file -Force
            }
            if ($problem) { throw "Could not sign ${file}: $problem" }
        } finally {
            Remove-Item -LiteralPath $unsigned -Force -ErrorAction SilentlyContinue
        }
    }

    if ($Certificate) {
        $cer = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Certificate)
        [IO.File]::WriteAllBytes($cer, $signer.Export([Security.Cryptography.X509Certificates.X509ContentType]::Cert))
        Write-Host "certificate written to $cer"
    }
} finally {
    if (-not $PfxFile) { Remove-Item -LiteralPath $pfx -Force -ErrorAction SilentlyContinue }
    if ($signer) { $signer.Dispose() }
}

# signtool's last exit code can be 2 although the signature is right (see above), and a pwsh
# step on GitHub ends with `exit $LASTEXITCODE`. The verdict is this script's, so say it.
exit 0
