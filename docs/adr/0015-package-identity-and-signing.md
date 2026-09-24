# ADR-0015: Package identity, signing and what a release publishes

Status: accepted (stage 8); amended in 0.28.0 (the *Amended* paragraphs below: a new, public
repository, and the release key rotated into an environment). Supersedes ADR-0004's identity
clause, which gave the one signing certificate one role; the package keeps what it said (its
subject never changes), the installer gets an identity of its own. Supersedes ADR-0012's "the
scripts remain in the release zip for now" and its build of the installer around the package.

## Context

- Windows derives a package's family from its publisher, the subject of the certificate it is
  signed with, and Mujina's home app is stored as `<family>!App`. A package signed under another
  subject is another app, and updating to it means a reinstall (ADR-0004).
- The package is self-signed (`CN=Mujina`), so users trust its certificate once, which Mujina
  Setup does behind one administrator prompt. Developer Mode stays under every option: the
  gamingHome capability's SCCD is unsigned (ADR-0004), whoever signs the package.
- The file users download is `Mujina-Setup-<tag>.exe` (`mujina-setup.exe` as built), signed with
  the same self-signed certificate.
  SmartScreen treats a self-signed file like an unsigned one, and Smart App Control "will block
  execution of unsigned files unless the file has a positive reputation"
  ([SmartScreen reputation](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation)).
  A certificate that chains to a trusted root would help the download, not the package.
- SignPath Foundation signs open-source projects for free, but "the code signing certificate is
  issued to SignPath Foundation", which then is the publisher, and every component must be
  under an OSI licence "without commercial dual-licensing"
  ([SignPath terms](https://signpath.org/terms.html)); Mujina's windows use Slint under its
  royalty-free licence. Put into the package, that identity would be SignPath's, not Mujina's,
  and leaving SignPath would change the family again.
- The repository is private until the stages of the plan are done. Its signing keys are
  repository secrets, and "any user with write access to your repository" can read them
  ([secure use reference](https://docs.github.com/en/actions/reference/security/secure-use)).
  An environment holds a secret for "only ... workflow jobs that reference the environment" and
  can "restrict which branches and tags can deploy" to it, but in a private repository
  environment secrets need GitHub Pro, Team or Enterprise, deployment branches and tags Pro or
  Team, and on Free, Pro and Team "required reviewers are only available for public
  repositories"
  ([deployments and environments](https://docs.github.com/en/actions/reference/workflows-and-actions/deployments-and-environments)).
  On GitHub Free, nothing can gate the release key while the repository is private.

  *Amended in 0.28.0:* the repository is public, and it is a new one. It starts with a single
  commit whose tree is the private repository's last `main`, in the middle of 0.28.0's
  development. Nothing else came along: not the git history, not the tags `v0.1.0` to `v0.27.0`
  and their releases, not the pull requests and issues, not the Actions runs with their logs
  and artifacts. The private repository was deleted. The CHANGELOG and these records remain the
  account of 0.1.0 to 0.27.0, which are not published any more; the [index](README.md) says
  what the stages named in them were. As a public repository on GitHub Free, this one can use
  environments with secrets, deployment tag rules and required reviewers (deployments and
  environments, above), and artifact
  attestations ([actions/attest](https://github.com/actions/attest)), so the release key is
  gated now (*Amended* under Decision).
- Until stage 8 the installer compiled its package in (`include_bytes!` in `build.rs`): building
  it needed the signed package, so the release workflow had six jobs, and a wrong path silently
  built an installer without one. The release also shipped a zip with `Install.ps1`,
  `Uninstall.ps1` and their `.cmd` starters, which had drifted from the installer.

## Decision

- **Two signatures, two identities.**
  - The **package** keeps Mujina's own constant `CN=Mujina` certificate, self-signed and done
    properly: every signature timestamped (RFC 3161), the installer checking for exactly the
    certificate it carries, a thumbprint and expiry on record and a rotation runbook
    ([docs/signing.md](../signing.md)). The family never changes, so there is never a forced
    reinstall.
  - The **installer** (`mujina-setup.exe`) gets a CA-chained signature from SignPath Foundation
    once the repository is public, after SignPath has confirmed that Slint's royalty-free licence
    is acceptable and the owner accepts SignPath shown as its publisher. Only the installer:
    with SignPath, a release makes a second signing request for the executable that carries the
    already-signed package. Until then, and for good if SignPath refuses, the installer is
    self-signed and the README explains the SmartScreen and Smart App Control warnings.

    *Amended in 0.28.0:* the repository is public now; the other conditions stand.
- **CI has an identity of its own:** CI packages carry `CN=Mujina Dev`, signed with a separate,
  low-value certificate (`DEV_SIGNING_PFX_*`). They are another family, install beside a release
  and never outrank it. Only `release.yml`'s sign job, which runs for a `v*` tag, uses the release
  key; the workflow code keeps it there, not GitHub (Context).

  *Amended in 0.28.0:* GitHub keeps it there too: the key is a secret of the `release`
  environment, which admits only `v*` tags (below).
- **The package is attached to the installer after linking.** Mujina Setup is built without it;
  `packaging/attach-payload.ps1` adds the signed package, its certificate and a few key=value
  lines (name, publisher, family, version, file name, thumbprint) as data resources
  (`RT_RCDATA`) with Windows' UpdateResource
  ([BeginUpdateResource](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-beginupdateresourcew)),
  and Setup reads them with FindResource. It runs before the installer is signed, so the
  signature covers them: the Authenticode image hash leaves out only the checksum, the
  certificate table's entry and the certificate table itself
  ([PE format](https://learn.microsoft.com/en-us/windows/win32/debug/pe-format);
  [Understanding executable file signing](https://learn.microsoft.com/en-us/windows/win32/secbp/understanding-pe-signatures)).
  The script checks what can go wrong at build time instead of on a device: the certificate
  must be the package's signer and its subject the manifest's Publisher; Windows derives the
  family (PackageFamilyNameFromId); the three are read back as data. Setup checks at run time
  only that the family follows from name and publisher: its certificate and the lines about it
  come from the same file, so comparing one with the other would prove nothing.
  `mujina-setup.exe --about` prints what it carries; a build straight from cargo says it
  carries none, and `build.rs` refuses the variables that used to embed a package.
- **A release is three jobs after its checks:** `build` (Windows, no secret: the unsigned
  package and Setup without it), `sign` (Windows, the release key, no cargo and nothing cargo
  built: it signs the package, attaches it and signs Setup, all timestamped) and `publish`
  (Ubuntu, the only job with `contents: write`). No job that runs cargo ever holds a key.
- **What a release publishes:** `Mujina-Setup-<tag>.exe`, the one file the README points users
  to; beside it, for experts, the bare `.msix` and, while self-signed, `Mujina.cer`; and
  `SHA256SUMS.txt`. No zip, no PowerShell scripts: `mujina-setup.exe --quiet` and
  `--uninstall --quiet` do what the scripts did, with the installer's own order and exit codes.
- **CI's `win-sign`** (main and manual runs, the development certificate) attaches the dev
  package to Setup too and uploads `Mujina-Setup-dev.exe`, so testers install a CI build the way
  users install a release.
- **`cargo xtask dist`** builds the same files on a developer's machine, with a certificate the
  developer supplies, through the same two scripts.
- **While the repository is private:** the release key stays a repository secret, which any
  workflow pushed to the repository could read; only people with write access can push one. On
  GitHub Pro or Team, a `release` environment with the deployment tag rule `v*` can hold it for
  the sign job alone ([docs/signing.md](../signing.md#gating-the-release-key)); on Free that
  waits for the repository to be public. Attestations and immutable releases wait for that too.

  *Amended in 0.28.0:* the release key is a secret of the `release` environment only; the
  repository has no secret for it. The environment admits only `v*` tags. `release.yml`'s `sign`
  job references it, and so does `keys`, with `deployment: false`, to test that the secrets are
  set.
  It has no required reviewers: a tag ruleset already lets only the owner create a `v*` tag, and
  with `deployment: false` a reviewer would approve every release twice
  ([docs/signing.md](../signing.md#gating-the-release-key)). `publish` attests Mujina Setup and
  the package with SLSA build provenance (`actions/attest`). Immutable releases never needed a
  public repository; they are a setting of the repository, outside this record.

  The key is new, too. The old one (thumbprint `C21089F232C29920F2A17D78E389E130F3FD589A`),
  which signed the releases up to 0.27.0, was a repository secret, and from 2026-09-20 to
  2026-09-23 it was handed to CI jobs that also ran cargo, and so ran the build scripts of
  Mujina's dependencies. No leak was known. But only the owner's device trusted that
  certificate, so replacing it cost one administrator prompt there and nothing anywhere else;
  it would never be cheaper. The new key keeps the subject `CN=Mujina`, so the package family
  `Mujina_k2veznmcx4n98` stays, and Mujina Setup asks once to trust the new certificate, since
  it trusts exactly the certificate it carries (ADR-0012). The owner made it, stored it
  straight in the environment, and keeps one encrypted backup offline: a secret cannot be read
  back, and without a copy any later move of the key would be another rotation, which on a
  public project prompts every user. The old key was not carried over; it ended with the old
  repository. The development key (`CN=Mujina Dev`) was made anew for this repository, the
  same way as before; its family, `Mujina_zexajcypj3ggw`, stays.

## Consequences

- The job that holds the key runs two short PowerShell scripts and signtool, nothing compiled
  from Mujina's dependencies. The release is shorter by two Windows jobs.
- Anyone can reproduce a release's shape locally (`cargo xtask dist`), and a CI build is
  installed exactly like a release.
- While the installer is self-signed, the administrator prompt says "Unknown publisher" for the
  real Setup and for a binary a same-user attacker put in its place; only a CA-chained signature
  on the installer makes that visible. Microsoft counts UAC as defense in depth, not as a
  security boundary
  ([Security Servicing Criteria for Windows](https://www.microsoft.com/en-us/msrc/windows-security-servicing-criteria));
  the installer's part is to trust nothing its unelevated side wrote (stage 1).
- Users who want the package without Setup take the `.msix` and `.cer` and do Setup's steps by
  hand; the README names Setup as the way.
- winget is out of scope until the installer has a CA-chained signature.
