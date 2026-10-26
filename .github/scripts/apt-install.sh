#!/usr/bin/env bash
# Installs apt packages on a GitHub-hosted Ubuntu runner without letting one
# bad moment hang the job. A plain `apt-get install` here has stalled until
# the job timeout more than once, and the runner log does not always say on
# what, so every common cause is bounded:
#   - a mirror that stops sending:      HTTP timeouts + apt's own retries
#   - a mirror that trickles or hangs:  a hard timeout per attempt, then retry
#   - a dpkg killed mid-install:        `dpkg --configure -a` before retrying
#   - another apt process holding locks: wait for it, but not forever
#   - man-db reindexing after install:  disabled; it can take minutes alone
#
# Usage: bash .github/scripts/apt-install.sh <package>...
set -euo pipefail

export DEBIAN_FRONTEND=noninteractive

sudo rm -f /var/lib/man-db/auto-update
cat <<'EOF' | sudo tee /etc/apt/apt.conf.d/80-ci > /dev/null
Acquire::Retries "3";
Acquire::http::Timeout "30";
Acquire::https::Timeout "30";
DPkg::Lock::Timeout "120";
EOF

# Recommends would pull in docs, fonts and desktop extras the build never
# touches; WebKit's tree is several hundred megabytes of them.
for attempt in 1 2 3; do
  if sudo -E timeout --kill-after=15 240 apt-get update -q &&
    sudo -E timeout --kill-after=15 300 apt-get install -y -q --no-install-recommends "$@"; then
    exit 0
  fi
  echo "::warning::apt attempt ${attempt} of 3 failed or stalled; retrying"
  sudo -E dpkg --configure -a || true
  sleep 15
done

echo "::error::apt-get could not install: $*"
exit 1
