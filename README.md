# ZapFast personal build

This branch holds only the automation for a personal ZapFast build. `main`
mirrors [crmne/zapfast](https://github.com/crmne/zapfast).

- `.github/workflows/personal-build.yml` runs every four hours. When the
  latest upstream release or an open pull request by the listed authors
  changes, it merges those pull requests onto the release, builds the macOS
  app (Apple silicon, ad-hoc signed) and publishes it as this fork's latest
  release. Pull requests that conflict are left out and named in the notes.
  Run it by hand from the Actions tab ("Run workflow", optionally forced).
- `mac/install-agent.sh` installs a launchd agent that checks hourly and
  installs a new build into `/Applications`, quitting and reopening ZapFast
  if it was running. `mac/install-agent.sh --remove` uninstalls it. Log:
  `~/Library/Logs/zapfast-personal-update.log`.

Edit `AUTHORS` in the workflow to change whose pull requests are included.
Add a `SYNC_TOKEN` secret (a token with the `workflow` scope) if keeping the
fork's `main` in sync fails because upstream changed a workflow file.
