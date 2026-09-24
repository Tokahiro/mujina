# Signing

Mujina's package is signed with a self-signed certificate. Windows derives the package family
from the certificate's subject, so the subject never changes (ADR-0004, ADR-0015): a package
signed under another subject is another app, and updating to it means a reinstall. Mujina Setup,
the file users download, carries the signed package and is signed itself: for now with the same
certificate, later with a CA-chained one of its own (ADR-0015).

## The certificate

| | |
|---|---|
| Subject | `CN=Mujina` (the repository variable `MSIX_PUBLISHER`) |
| Thumbprint (SHA-1) | `3602B663B634E9E0D145F002C7D162C673752CBB` |
| SHA-256 | `9B9D9ACFC67AEA225F62C204F83BE7B8863C74DEAC4A29EF612F3B0549A6C233` |
| Valid | 2026-09-24 to **2036-09-24** |
| Key | RSA 3072, code signing only (EKU `1.3.6.1.5.5.7.3.3`), not a CA |
| Private key | the secrets `SIGNING_PFX_BASE64` and `SIGNING_PFX_PASSWORD` of the `release` environment, used only by the release workflow's `sign` job ([how GitHub keeps them there](#gating-the-release-key)), and one encrypted backup the maintainer keeps offline |

This certificate is new with 0.28.0. The one of the same subject that signed 0.27.0 and earlier,
thumbprint `C21089F232C29920F2A17D78E389E130F3FD589A`, is no longer used (ADR-0015);
[Rotating the certificate](#rotating-the-certificate) says how to remove it from a machine.

Read these from any release's `Mujina.cer`:

```powershell
$c = [Security.Cryptography.X509Certificates.X509Certificate2]::new('Mujina.cer')
$c.Subject; $c.Thumbprint; $c.NotAfter
```

## How builds are signed

`cargo xtask package` builds an unsigned package, and `cargo build --release -p mujina-setup`
Mujina Setup without it. Two scripts do the rest, in jobs that run no cargo, run nothing cargo
built, and check out nothing but these two scripts, so the key never shares a machine with the
build scripts of Mujina's dependencies:

- `packaging/sign.ps1` writes the PFX to the runner's temporary folder, signs with signtool from
  the newest Windows SDK (SHA-256), checks that the signature is by exactly that certificate
  and timestamped, and deletes the PFX again.
- `packaging/attach-payload.ps1` attaches the signed package, its certificate and a few lines
  about them (name, publisher, family, version, file name, thumbprint) to Mujina Setup as data
  resources, with Windows' UpdateResource. It first checks that the certificate is the
  package's signer and that its subject is the manifest's Publisher, and afterwards reads the
  three back as data, without running Setup. It runs before Setup is signed: the signature then
  covers them, since the Authenticode hash leaves out only the checksum and the certificate
  table ([PE format](https://learn.microsoft.com/en-us/windows/win32/debug/pe-format)).
  `mujina-setup.exe --about | Out-String` shows what a Setup carries.

- **Releases** (`release.yml`, on a `v*` tag): `build` packages and builds Setup without its
  package; `sign` signs the package (writing `Mujina.cer`), attaches it to Setup and signs Setup;
  `publish` writes `SHA256SUMS.txt`, attests Setup and the package with SLSA build provenance
  ([actions/attest](https://github.com/actions/attest)), and creates the release with
  `Mujina-Setup-<tag>.exe`, the package, `Mujina.cer` and the checksums. Only `sign` uses the
  release key; `keys` receives it only to test that it is set, and runs nothing else. CI never
  gets it.
  `gh attestation verify <file> --repo Tokahiro/mujina --signer-workflow Tokahiro/mujina/.github/workflows/release.yml`
  checks that a downloaded file was built by this repository's release workflow; `--repo`
  alone accepts an attestation from any workflow of the repository
  ([gh attestation verify](https://cli.github.com/manual/gh_attestation_verify),
  [using artifact attestations](https://docs.github.com/en/actions/how-tos/secure-your-work/use-artifact-attestations/use-artifact-attestations)).
- **CI** (`ci.yml`): every package is built unsigned. On `main`, and in runs started by hand
  (Actions → CI → Run workflow, on any branch), `win-sign` signs it with the development
  certificate below, once clippy and the tests have passed, attaches it to Setup and signs
  that too. It uploads `Mujina-Setup-dev.exe` (`setup-dev`), and the package with
  `Mujina-Dev.cer` (`msix-signed`). Without the development secrets, the job says so and leaves
  the package unsigned.
  On those runs `win-package` also attaches the unsigned package to Setup with a throwaway
  certificate made in memory, without any key, and uploads nothing it made: the script runs on
  every push to `main`, not first in a release's `sign` job.
- **On your own machine**: `cargo xtask dist --pfx <file>` builds the same files into
  `target/dist`, with a certificate you supply (its password in `SIGNING_PFX_PASSWORD`, its
  subject in `MSIX_PUBLISHER`, `CN=Mujina Dev` if unset). Use a development certificate, never
  the release one: signtool keeps a copy of the key on the machine (below).

signtool keeps a copy of the private key in the key store of the user it runs as, one per call.
A hosted runner is thrown away after the job; on a machine that is kept, sign from the
certificate store instead of with `sign.ps1`.

## Gating the release key

The release key is kept as the secrets `SIGNING_PFX_BASE64` and `SIGNING_PFX_PASSWORD` of the
environment `release`. There are no repository secrets of these names: "any user with write
access to your repository" can read every repository secret, for instance with a workflow
pushed to a branch
([secure use reference](https://docs.github.com/en/actions/reference/security/secure-use)).
An environment narrows that: "secrets stored in an environment are only available to workflow
jobs that reference the environment", and its deployment branches and tags "restrict which
branches and tags can deploy to the environment"
([deployments and environments](https://docs.github.com/en/actions/reference/workflows-and-actions/deployments-and-environments)).

- **The environment** has a single deployment rule: Settings → Environments → `release` →
  *Deployment branches and tags* → *Selected branches and tags*, a rule with the ref type *Tag*
  and the pattern `v*`
  ([managing environments](https://docs.github.com/en/actions/how-tos/deploy/configure-and-manage-deployments/manage-environments)).
  A job that deploys to it runs only for a `v*` tag. GitHub's documentation does not say
  whether the rule also holds for a job that only references the environment with
  `deployment: false`, as `keys` does; `release.yml` runs only for `v*` tags either way.
- **In `release.yml`** the `sign` job references it (`environment: release`) and is the only job
  that uses the key. The `keys` job references it too, since it could not see whether the
  secrets are set otherwise, with `deployment: false`: it deploys nothing, so GitHub records no
  deployment for it. GitHub names "Access control—restrict which branches can use certain
  secrets via environment branch policies, without deployment tracking" as a use of that
  setting
  ([control deployments](https://docs.github.com/en/actions/how-tos/deploy/configure-and-manage-deployments/control-deployments)).
- **No required reviewers**, on purpose. A tag ruleset lets only the maintainer create, move or
  delete a `v*` tag, and pushing one is the decision to release. A reviewer would be the same
  person approving that decision again, twice per release: with `deployment: false`, "required
  reviewers still apply" (same page), so `keys` would wait for an approval as well as `sign`.
  The tag rule and the ruleset are the gate.
- **The secrets are set** straight into the environment, with
  `gh secret set <NAME> --env release`, never with `--body` or through the clipboard
  ([step 2 below](#rotating-the-certificate)).

## Timestamps

Every signature carries an RFC 3161 timestamp from DigiCert (`http://timestamp.digicert.com`).
If DigiCert fails, the file is signed again with Sectigo's (`http://timestamp.sectigo.com`).
Both are plain HTTP, which is what they offer; the timestamp is itself signed.

## Expiry

A signature without a timestamp is checked against the current time: once the certificate has
expired, Windows no longer installs the package. Every release here is timestamped and keeps
installing, because Windows checks the certificate against the time of signing. (Versions up to
0.27.0 were signed without a timestamp, with the previous certificate; they were published only
in this repository's private predecessor and are no longer available.)

## The development certificate

CI packages carry the publisher `CN=Mujina Dev` (what `cargo xtask package` uses when
`MSIX_PUBLISHER` is not set). That is another package family: a CI build installs beside the
release and never updates or outranks it, whatever its revision.

| | |
|---|---|
| Subject | `CN=Mujina Dev` |
| Thumbprint (SHA-1) | `CECCE0A92E87156BF0C1CA913270BA21AF03A25D` |
| SHA-256 | `853F0C8E3962FA461AC16BF44AAC1F666CA7CFB69B594B8C79A61EA28EF4B55C` |
| Valid | 2026-09-24 to 2036-09-24 |
| Package family | `Mujina_zexajcypj3ggw` |
| Private key | the repository secrets `DEV_SIGNING_PFX_BASE64` and `DEV_SIGNING_PFX_PASSWORD`, and nowhere else |

It is worth little: only testers who trusted `Mujina-Dev.cer` accept packages signed with it.
It is made like the release certificate (step 1 below) but with
`-Subject 'CN=Mujina Dev' -FriendlyName 'Mujina Dev'`, and stored as the two `DEV_` secrets of
the repository, not of the `release` environment. Its password is random and never shown, and
no copy of the PFX is kept:

```powershell
$bytes = New-Object byte[] 24; [Security.Cryptography.RandomNumberGenerator]::Create().GetBytes($bytes)
$plain = [Convert]::ToBase64String($bytes) -replace '[+/=]', ''
$pfx = Join-Path $env:TEMP 'mujina-dev.pfx'
Export-PfxCertificate -Cert Cert:\CurrentUser\My\<thumbprint> -FilePath $pfx -Password (ConvertTo-SecureString $plain -AsPlainText -Force)
$OutputEncoding = [Console]::InputEncoding = New-Object System.Text.UTF8Encoding $false
$plain | gh secret set DEV_SIGNING_PFX_PASSWORD --repo Tokahiro/mujina
[Convert]::ToBase64String([IO.File]::ReadAllBytes($pfx)) | gh secret set DEV_SIGNING_PFX_BASE64 --repo Tokahiro/mujina
Remove-Item -LiteralPath $pfx; Remove-Item Cert:\CurrentUser\My\<thumbprint> -DeleteKey
Remove-Variable plain, bytes
```

The encoding line keeps a byte order mark out of both secrets; step 2 of
[Rotating the certificate](#rotating-the-certificate) explains why.

If the key is lost or may have leaked, make a new one the same way and replace both secrets.
The subject stays, so the family stays; Mujina Setup for a CI build asks once to trust the new
certificate.

To install a CI build, run `Mujina-Setup-dev.exe` from the run's `setup-dev` artifact: it asks
once for administrator rights to trust the development certificate, as a release's Setup does for
its own. It installs beside a release, as another app; its Settings entry and its removal are its
own.

## Rotating the certificate

Rotate before the certificate expires, or at once if the private key may have leaked.

1. Create the new certificate with the **same subject** (Microsoft's recipe for package signing
   certificates, with a longer life than the default year):

   ```powershell
   New-SelfSignedCertificate -Type Custom -KeyUsage DigitalSignature `
     -KeyAlgorithm RSA -KeyLength 3072 -NotAfter (Get-Date).AddYears(10) `
     -CertStoreLocation Cert:\CurrentUser\My -Subject 'CN=Mujina' -FriendlyName 'Mujina' `
     -TextExtension @('2.5.29.37={text}1.3.6.1.5.5.7.3.3', '2.5.29.19={text}')
   ```

2. Export it with a new password and store it straight in the `release` environment:

   ```powershell
   $pfx = Join-Path $env:TEMP 'mujina.pfx'
   $password = Read-Host -AsSecureString
   Export-PfxCertificate -Cert Cert:\CurrentUser\My\<thumbprint> -FilePath $pfx -Password $password
   $OutputEncoding = [Console]::InputEncoding = New-Object System.Text.UTF8Encoding $false
   [Convert]::ToBase64String([IO.File]::ReadAllBytes($pfx)) | gh secret set SIGNING_PFX_BASE64 --env release --repo Tokahiro/mujina
   gh secret set SIGNING_PFX_PASSWORD --env release --repo Tokahiro/mujina
   ```

   `$pfx` is a full path on purpose: .NET's `ReadAllBytes` resolves a relative name against the
   process's folder, which Windows PowerShell does not change with `Set-Location`, so a bare
   `'mujina.pfx'` could upload another file than the one just exported.
   The first `gh secret set` reads the key from the pipe and removes only a trailing line break;
   the second asks for the password at a prompt of its own
   ([`gh secret set`](https://github.com/cli/cli/blob/v2.101.0/pkg/cmd/secret/set/set.go)).
   The encoding line comes first because Windows PowerShell 5.1 puts a UTF-8 byte order mark
   (the bytes EF BB BF) in front of whatever it pipes to a program when the console's input code
   page is UTF-8 (`chcp 65001`), or when `$OutputEncoding` is a UTF-8 encoding with a byte order
   mark (for example `[Text.Encoding]::UTF8` set in a profile). The secret would then start with
   those bytes, and the key would no longer decode. The line sets both to UTF-8 without a byte
   order mark, which passes the bytes unchanged.
   Never pass either with `--body`, which puts it on the command line, or through the
   clipboard.
3. Keep one encrypted backup offline: the file at `$pfx` and its password, on encrypted storage
   that is not connected; it replaces any backup of an earlier key. A secret cannot be read back,
   so without it any later move of the key (another environment, another repository) would be
   another rotation, which costs every user an administrator prompt. Then delete the file,
   `Remove-Item -LiteralPath $pfx`, and remove the certificate with its private key from
   `Cert:\CurrentUser\My`: `Remove-Item Cert:\CurrentUser\My\<thumbprint> -DeleteKey`
   ([Certificate provider](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.security/about/about_certificate_provider?view=powershell-5.1)).
4. Update the table above.
5. The next release's installer carries the new certificate and asks for administrator rights
   once to trust it: it looks for exactly the certificate it carries (ADR-0012, amendment of
   stage 1), so the old one with the same name does not count. Say so in the CHANGELOG.

The old certificate stays trusted on users' machines until it is removed there, and after a leak
it has to go. Until the installer can remove it, the release notes explain how:
`Get-ChildItem Cert:\LocalMachine\TrustedPeople | Where-Object Thumbprint -eq <old> | Remove-Item`
in an administrator PowerShell.

Sources: [Create a certificate for package signing](https://learn.microsoft.com/en-us/windows/msix/package/create-certificate-package-signing),
[Signing overview](https://learn.microsoft.com/en-us/windows/msix/package/signing-package-overview),
[SignTool](https://learn.microsoft.com/en-us/windows/win32/seccrypto/signtool),
[DigiCert's timestamp server](https://knowledge.digicert.com/general-information/rfc3161-compliant-time-stamp-authority-server),
[Sectigo's timestamp server](https://www.sectigo.com/resource-library/time-stamping-server).
