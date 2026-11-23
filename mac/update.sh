#!/bin/bash
# Installs the newest personal build from this fork's releases into
# /Applications when the installed app is not that build. Run hourly by
# launchd (see install-agent.sh). Downloads with curl carry no quarantine.
set -euo pipefail

REPO=thize/zapfast
ASSET=ZapFast-macos-arm64.zip
APP=/Applications/ZapFast.app
BASE="https://github.com/$REPO/releases/latest/download"
WORK="$HOME/Library/Caches/ZapFastPersonal"

log() { echo "$(date '+%Y-%m-%d %H:%M:%S') $*"; }

mkdir -p "$WORK"
latest=$(curl -fsSL --max-time 30 "$BASE/fingerprint.txt" | tr -d '[:space:]') || { log "offline"; exit 0; }
installed=$(tr -d '[:space:]' < "$APP/Contents/Resources/personal-build.txt" 2>/dev/null || true)
if [ "$latest" = "$installed" ]; then
    exit 0
fi

log "installing $latest (installed: ${installed:-none})"
rm -rf "$WORK/new" "$WORK/new.zip"
mkdir -p "$WORK/new"
curl -fsSL --max-time 600 -o "$WORK/new.zip" "$BASE/$ASSET"
ditto -x -k "$WORK/new.zip" "$WORK/new"
new="$WORK/new/ZapFast.app"
# A release published between the two downloads: try again next hour.
if [ "$(tr -d '[:space:]' < "$new/Contents/Resources/personal-build.txt")" != "$latest" ]; then
    log "download does not match $latest, retrying later"
    exit 0
fi
codesign --verify --strict "$new"

running=false
if pgrep -xq zapfast; then
    running=true
    osascript -e 'quit app "ZapFast"' || true
    for _ in $(seq 30); do pgrep -xq zapfast || break; sleep 1; done
    pkill -x zapfast 2>/dev/null || true
fi

rm -rf "$APP.old"
[ -d "$APP" ] && mv "$APP" "$APP.old"
ditto "$new" "$APP"
rm -rf "$APP.old" "$WORK/new" "$WORK/new.zip"

if $running; then
    open -g "$APP"
fi
osascript -e "display notification \"Instalada a build $latest\" with title \"ZapFast atualizado\"" || true
log "installed $latest"
