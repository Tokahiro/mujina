<#
.SYNOPSIS
    Attaches Mujina's signed package to a built Mujina Setup. Needs no cargo.

.DESCRIPTION
    Adds the package, its certificate and a few lines about them as RT_RCDATA resources. Run it
    before signing: the Authenticode hash leaves out only the checksum and the certificate
    table, so the signature covers them (Microsoft's PE format reference). A certificate that does
    not match the package fails here, not on a user's device. The result is read back as data,
    never run: this runs where the signing key is (docs/signing.md).

.PARAMETER Setup
    The built mujina-setup.exe, unsigned. Left as it is when -Output is given.

.PARAMETER Msix
    The signed package.

.PARAMETER Certificate
    The package's certificate (.cer), as packaging/sign.ps1 -Certificate writes it.

.PARAMETER Output
    Where the Setup with its package goes. Without it, -Setup itself is changed.

.PARAMETER AllowUnsignedPackage
    For a dry run with an unsigned package: the signer is not checked, and the Setup cannot
    install its package.

.EXAMPLE
    pwsh packaging/attach-payload.ps1 -Setup target/release/mujina-setup.exe -Msix msix/Mujina_0.28.0.0_x64.msix -Certificate msix/Mujina.cer -Output out/Mujina-Setup-v0.28.0.exe
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)] [string] $Setup,
    [Parameter(Mandatory)] [string] $Msix,
    [Parameter(Mandatory)] [string] $Certificate,
    [string] $Output,
    [switch] $AllowUnsignedPackage
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 3.0

Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
if (-not ('Mujina.Resources' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;

namespace Mujina {
    public static class Resources {
        [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
        public static extern IntPtr BeginUpdateResourceW(string file, bool deleteExisting);

        [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
        public static extern bool UpdateResourceW(IntPtr update, IntPtr type, string name, ushort language, byte[] data, uint size);

        [DllImport("kernel32.dll", SetLastError = true)]
        public static extern bool EndUpdateResourceW(IntPtr update, bool discard);

        // PACKAGE_ID of appmodel.h: the version is a UINT64, the strings are PWSTR.
        [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
        public struct PackageId {
            public uint Reserved;
            public uint ProcessorArchitecture;
            public ulong Version;
            public string Name;
            public string Publisher;
            public string ResourceId;
            public string PublisherId;
        }

        [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
        static extern IntPtr LoadLibraryExW(string file, IntPtr reserved, uint flags);

        [DllImport("kernel32.dll")]
        static extern bool FreeLibrary(IntPtr module);

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode)]
        static extern IntPtr FindResourceW(IntPtr module, string name, IntPtr type);

        [DllImport("kernel32.dll")]
        static extern uint SizeofResource(IntPtr module, IntPtr resource);

        [DllImport("kernel32.dll")]
        static extern IntPtr LoadResource(IntPtr module, IntPtr resource);

        [DllImport("kernel32.dll")]
        static extern IntPtr LockResource(IntPtr loaded);

        // LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE: the file is mapped to read
        // its resources, and none of its code runs.
        const uint AsData = 0x2 | 0x20;

        // A raw-data resource of an executable, read without running it; null if it has none.
        public static byte[] ReadResource(string file, string name) {
            IntPtr module = LoadLibraryExW(file, IntPtr.Zero, AsData);
            if (module == IntPtr.Zero) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
            try {
                IntPtr found = FindResourceW(module, name, (IntPtr)10);
                if (found == IntPtr.Zero) return null;
                uint size = SizeofResource(module, found);
                IntPtr data = LockResource(LoadResource(module, found));
                if (data == IntPtr.Zero) return null;
                var bytes = new byte[size];
                Marshal.Copy(data, bytes, 0, (int)size);
                return bytes;
            } finally {
                FreeLibrary(module);
            }
        }

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode)]
        static extern int PackageFamilyNameFromId(ref PackageId id, ref uint length, StringBuilder familyName);

        // The family Windows derives from a package's name and publisher; null if it refuses them.
        public static string FamilyName(string name, string publisher) {
            var id = new PackageId { Name = name, Publisher = publisher };
            uint length = 65;
            var family = new StringBuilder((int)length);
            return PackageFamilyNameFromId(ref id, ref length, family) == 0 ? family.ToString() : null;
        }
    }
}
'@
}

# RT_RCDATA, raw data: MAKEINTRESOURCE(10). The resources are language neutral.
$RawData = [IntPtr]10
$Neutral = [uint16]0

function Resolve-Full([string] $path) {
    $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($path)
}

$Setup = Resolve-Full $Setup
$Msix = Resolve-Full $Msix
$Certificate = Resolve-Full $Certificate
foreach ($file in $Setup, $Msix, $Certificate) {
    if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { throw "$file not found" }
}
if ((Get-AuthenticodeSignature -LiteralPath $Setup).SignerCertificate) {
    throw "$Setup is signed already: attach the package first, then sign (packaging/sign.ps1)"
}

$zip = [IO.Compression.ZipFile]::OpenRead($Msix)
try {
    $entry = $zip.GetEntry('AppxManifest.xml')
    if (-not $entry) { throw "$Msix has no AppxManifest.xml" }
    $reader = [IO.StreamReader]::new($entry.Open())
    try { [xml] $manifest = $reader.ReadToEnd() } finally { $reader.Dispose() }
} finally {
    $zip.Dispose()
}
$identity = $manifest.Package.Identity
$name = $identity.Name
$publisher = $identity.Publisher
$version = $identity.Version
if (-not $name -or -not $publisher -or $version -notmatch '^\d+\.\d+\.\d+\.\d+$') {
    throw "$Msix has no complete Identity (Name '$name', Publisher '$publisher', Version '$version')"
}

$cer = [Security.Cryptography.X509Certificates.X509Certificate2]::new($Certificate)
try {
    if ($cer.Subject -ne $publisher) {
        throw "The certificate is $($cer.Subject), but the package's publisher is $publisher."
    }
    $signer = (Get-AuthenticodeSignature -LiteralPath $Msix).SignerCertificate
    if ($signer -and $signer.Thumbprint -ne $cer.Thumbprint) {
        throw "The package is signed by $($signer.Subject) ($($signer.Thumbprint)), not by this certificate ($($cer.Thumbprint))."
    }
    if (-not $signer -and -not $AllowUnsignedPackage) {
        throw "$Msix is not signed: sign it first (packaging/sign.ps1)."
    }
    $thumbprint = $cer.Thumbprint
    $cerBytes = $cer.Export([Security.Cryptography.X509Certificates.X509ContentType]::Cert)
} finally {
    $cer.Dispose()
}

$family = [Mujina.Resources]::FamilyName($name, $publisher)
if (-not $family) { throw "Windows derives no package family from '$name' and '$publisher'." }
$msixName = Split-Path -Leaf $Msix
$about = @(
    "name=$name"
    "publisher=$publisher"
    "family=$family"
    "version=$version"
    "msix=$msixName"
    "certificate=$thumbprint"
) -join "`r`n"
$aboutBytes = [Text.UTF8Encoding]::new($false).GetBytes($about + "`r`n")

$target = $Setup
if ($Output) {
    $target = Resolve-Full $Output
    $folder = Split-Path -Parent $target
    if ($folder) { New-Item -ItemType Directory -Force -Path $folder | Out-Null }
    Copy-Item -LiteralPath $Setup -Destination $target -Force
}

$update = [Mujina.Resources]::BeginUpdateResourceW($target, $false)
if ($update -eq [IntPtr]::Zero) {
    throw "BeginUpdateResource($target) failed: $([ComponentModel.Win32Exception]::new([Runtime.InteropServices.Marshal]::GetLastWin32Error()).Message)"
}
$written = $false
try {
    $parts = [ordered]@{
        MUJINA_MSIX  = [IO.File]::ReadAllBytes($Msix)
        MUJINA_CER   = $cerBytes
        MUJINA_ABOUT = $aboutBytes
    }
    foreach ($part in $parts.GetEnumerator()) {
        $bytes = [byte[]] $part.Value
        if (-not [Mujina.Resources]::UpdateResourceW($update, $RawData, $part.Key, $Neutral, $bytes, [uint32] $bytes.Length)) {
            throw "UpdateResource($($part.Key)) failed: $([ComponentModel.Win32Exception]::new([Runtime.InteropServices.Marshal]::GetLastWin32Error()).Message)"
        }
    }
    if (-not [Mujina.Resources]::EndUpdateResourceW($update, $false)) {
        throw "EndUpdateResource failed: $([ComponentModel.Win32Exception]::new([Runtime.InteropServices.Marshal]::GetLastWin32Error()).Message)"
    }
    $written = $true
} finally {
    # On a failure nothing is written.
    if (-not $written) { [void][Mujina.Resources]::EndUpdateResourceW($update, $true) }
}

# Read back as Windows will find them, without running the file: the signing key is here.
foreach ($part in $parts.GetEnumerator()) {
    $back = [Mujina.Resources]::ReadResource($target, $part.Key)
    if (-not $back -or [Convert]::ToBase64String($back) -ne [Convert]::ToBase64String([byte[]] $part.Value)) {
        throw "$target does not carry $($part.Key) as written"
    }
}
Write-Host $about
Write-Host "attached $msixName ($family) to $target"
exit 0
