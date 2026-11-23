#!/bin/bash
# Installs the hourly launchd agent that keeps /Applications/ZapFast.app on
# the newest personal build. Run again to update; pass --remove to uninstall.
set -euo pipefail

LABEL=local.zapfast.personal-update
DIR="$HOME/Library/Application Support/ZapFastPersonal"
PLIST="$HOME/Library/LaunchAgents/$LABEL.plist"
LOG="$HOME/Library/Logs/zapfast-personal-update.log"

launchctl bootout "gui/$(id -u)/$LABEL" 2>/dev/null || true
if [ "${1:-}" = "--remove" ]; then
    rm -f "$PLIST"
    rm -rf "$DIR"
    echo "removed"
    exit 0
fi

mkdir -p "$DIR" "$(dirname "$PLIST")"
cp "$(dirname "$0")/update.sh" "$DIR/update.sh"
chmod 755 "$DIR/update.sh"

cat > "$PLIST" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key><string>$LABEL</string>
    <key>ProgramArguments</key>
    <array><string>/bin/bash</string><string>$DIR/update.sh</string></array>
    <key>StartInterval</key><integer>3600</integer>
    <key>RunAtLoad</key><true/>
    <key>StandardOutPath</key><string>$LOG</string>
    <key>StandardErrorPath</key><string>$LOG</string>
</dict>
</plist>
EOF

launchctl bootstrap "gui/$(id -u)" "$PLIST"
echo "installed $LABEL, log at $LOG"
