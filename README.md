# ZapFast personal build

This branch holds only the automation for a personal ZapFast build. `main`
mirrors [crmne/zapfast](https://github.com/crmne/zapfast).

`.github/workflows/personal-build.yml` runs every four hours. When the
latest upstream release or an open pull request by the listed authors
changes, it merges those pull requests onto the release, points the app's
updater and packaging at this fork (`patch-updater.sh`), builds the same
downloads as an upstream release, and publishes them as a release of this
fork, signed with this fork's update key:

- macOS: universal disk image (Apple silicon and Intel).
- Windows x64 and arm64: portable zip and Windows Installer package (msi).
- Linux x64 and arm64: archive, AppImage, deb and rpm; x64 Flatpak bundle.
- `checksums.txt` and its signature, and `packaging-checksums.txt`.

Pull requests that conflict are left out and named in the notes. Run it by
hand from the Actions tab ("Run workflow", optionally forced).

A personal build is versioned `<upstream>-personal.<run>`, and its updater
reads this fork's releases only, so the app offers each new personal build
itself, as it would an official release. The first install is by hand: on
macOS open the newest disk image and drag ZapFast to Applications; on
Windows run the msi, or unzip the zip where ZapFast should live and run
`zapfast.exe` (its first start copies the existing account data beside it).

Differences from upstream's downloads:

- Nothing is signed by Apple or Microsoft: this fork has no Apple Developer
  or Azure Artifact Signing account. Windows SmartScreen warns about the zip
  and the msi.
- The deb and rpm carry the version as `~personal.N` inside; their file
  names use `-personal.N`, since a release file name cannot hold a tilde.
- native-packages takes only `-alpha.N`, `-beta.N` and `-rc.N` prereleases;
  the workflow lets the installed gem take `-personal.N` too.
- No `*-packaging.tar.xz`: native-packages makes the AUR and Homebrew
  recipes only for stable versions, and every personal build is a
  prerelease.
- Each release also carries a `macos-arm64` copy of the universal disk image:
  builds up to 0.21.3-personal.5 update from that name. It can go once every
  Mac runs a later build.

- `update-public-key.hex` is the key personal builds trust. Its private half
  is the `UPDATE_SIGNING_KEY` secret, backed up outside GitHub. Losing it
  means the next build must be installed by hand.
- The app is signed with the self-made "ZapFast Assinatura" certificate
  (secrets `MACOS_CERT_P12`, base64, and `MACOS_CERT_PASSWORD`), so the
  Keychain keeps trusting it across updates instead of asking for the
  password after each one.
- Edit `AUTHORS` in the workflow to change whose pull requests are included.
- Add a `SYNC_TOKEN` secret (a token with the `workflow` scope) if keeping
  the fork's `main` in sync fails because upstream changed a workflow file.
