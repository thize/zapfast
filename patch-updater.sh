#!/bin/bash
# Points a ZapFast checkout's self-updater and packaging at this fork and
# gives it the personal build's version. Run from the checkout:
#
#   patch-updater.sh <version> <public-key.hex>
#
# The version carries a pre-release suffix (0.21.3-personal.42), so the app
# reads this fork's release list and takes the highest version above its own.
# macOS updates from the universal disk image, as upstream's do.
set -euo pipefail

version="$1"
key="$2"

export VERSION="$version"
perl -0pi -e 's{"crmne/zapfast"}{"thize/zapfast"} or die "updates.rs: repository not found\n"' src/updates.rs
perl -0pi -e 's{\n(\s*)\.\.UpdateConfig::new\(}{\n$1prereleases: fastframe_update::Prereleases::WhenRunningPrerelease,\n$1..UpdateConfig::new(} or die "updates.rs: UpdateConfig::new not found\n"' src/updates.rs
# Only this fork's key: releases signed by upstream are not personal builds.
cp "$key" assets/update-public-key.hex
cp "$key" assets/update-public-key-next.hex
perl -0pi -e 's{^version = "[^"]*"}{version = "$ENV{VERSION}"}m or die "Cargo.toml: version not found\n"' Cargo.toml
perl -0pi -e 's{(\nname = "zapfast"\nversion = )"[^"]*"}{$1"$ENV{VERSION}"} or die "Cargo.lock: zapfast not found\n"' Cargo.lock
# The packaging recipes download this fork's release files, not upstream's.
perl -0pi -e 's{^(\s*repository:\s*)crmne/zapfast\s*$}{$1thize/zapfast}m or die "native-packages.yaml: release repository not found\n"' native-packages.yaml
perl -0pi -e 's{github\.com/crmne/zapfast/archive/}{github.com/thize/zapfast/archive/}g or die "native-packages.yaml: source URL not found\n"' native-packages.yaml

git diff --stat
