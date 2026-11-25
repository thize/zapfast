# ZapFast personal build

This branch holds only the automation for a personal ZapFast build. `main`
mirrors [crmne/zapfast](https://github.com/crmne/zapfast).

`.github/workflows/personal-build.yml` runs every four hours. When the
latest upstream release, an open pull request by the workflow's `AUTHORS`,
one listed in `extra-prs.txt`, a branch of this fork listed in
`extra-branches.txt` (work not yet offered upstream), or a fix-up in
`fixups/` changes, it merges those pull requests and then those branches
onto the release, points the app's
updater and packaging at this fork (`patch-updater.sh`), builds the same
downloads as an upstream release, and publishes them as a release of this
fork, signed with this fork's update key:

- macOS: universal disk image (Apple silicon and Intel).
- Windows x64 and arm64: portable zip and Windows Installer package (msi).
- Linux x64 and arm64: archive, AppImage, deb and rpm; x64 Flatpak bundle.
- `checksums.txt` and its signature, and `packaging-checksums.txt`.

Nothing is left out quietly. When a pull request conflicts with the release
or with those merged before it (in number order), or a fix-up no longer
applies, the run stops before building and its summary names them; GitHub
then emails the failed run. The one exception is a conflict only in the
website's docs (`docs/`): the pull request is merged without those docs
changes, since the app does not ship them. Pull requests that merge cleanly
but do not compile together fail the builds, so nothing is published either.

`fixups/` holds small patches that make pull requests build together, named
after them (`853+889.patch`). One applies after all the merges, when every
pull request in its name is in the build:

- `853+889.patch`: #889's reaction notifications call
  `Notifications::allow` the way it was before #853, which takes a `Quiet`
  instead of a bool. Reactions wait as messages do.
- `820+1168.patch`: a test of #820 builds a sticker without the `audio`
  field #1168 adds. Only the tests need it.

Run it by hand from the Actions tab ("Run workflow", optionally forced).

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
- Edit `AUTHORS` in the workflow to change whose pull requests are included,
  and `extra-prs.txt` for single pull requests by anyone else. One that is
  merged or closed upstream drops out, and the release notes say so.
- List a branch of this fork in `extra-branches.txt` to try a change in the
  personal build before opening a pull request for it. Once it is a pull
  request by one of the `AUTHORS`, take it off the list.
- Add a `SYNC_TOKEN` secret (a token with the `workflow` scope) if keeping
  the fork's `main` in sync fails because upstream changed a workflow file.
