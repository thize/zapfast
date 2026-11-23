# ZapFast personal build

This branch holds only the automation for a personal ZapFast build. `main`
mirrors [crmne/zapfast](https://github.com/crmne/zapfast).

`.github/workflows/personal-build.yml` runs every four hours. When the
latest upstream release or an open pull request by the listed authors
changes, it merges those pull requests onto the release, points the app's
updater at this fork (`patch-updater.sh`), builds the macOS app (Apple
silicon, ad-hoc signed) and publishes it as a release of this fork, signed
with this fork's update key. Pull requests that conflict are left out and
named in the notes. Run it by hand from the Actions tab ("Run workflow",
optionally forced).

A personal build is versioned `<upstream>-personal.<run>`, and its updater
reads this fork's releases only, so the app offers each new personal build
itself, as it would an official release. The first install is by hand: open
the newest release's disk image and drag ZapFast to Applications.

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
