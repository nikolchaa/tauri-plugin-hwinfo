#!/usr/bin/env bash
# Installs apt packages on a GitHub-hosted Ubuntu runner without letting one
# bad moment hang the job. Bare `apt-get` has stalled runs into their job
# limits, and the step log needs a sign-in to read, so every stage is bounded
# and a failed attempt reports what it was stuck on as a public annotation:
#   - a mirror that stops sending:       HTTP timeouts + apt's own retries
#   - a mirror that crawls:              a hard limit per attempt, then retry;
#                                        downloaded packages stay cached, and
#                                        the index is fetched only once
#   - a dpkg killed mid-install:         `dpkg --configure -a` before retrying
#   - another apt process holding locks: wait for it, but not forever
#   - man-db reindexing after install:   disabled; it can take minutes alone
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

log="$(mktemp)"

# One apt-get command under a hard time limit, its output kept for `report`.
apt() {
  local limit=$1
  shift
  sudo -E timeout --kill-after=15 "$limit" apt-get "$@" 2>&1 | tee "$log"
}

report() {
  local tail
  tail="$(tail -n 4 "$log" | tr '\n' ' ' | cut -c1-400)"
  echo "::warning::apt attempt $1 of 3 failed during '$2': ${tail}"
}

# Recommends would pull in docs, fonts and desktop extras the build never
# touches; WebKit's tree is several hundred megabytes of them.
indexed=false
for attempt in 1 2 3; do
  if ! $indexed; then
    if apt 300 update -q; then
      indexed=true
    else
      report "$attempt" update
      sleep 15
      continue
    fi
  fi
  if apt 420 install -y -q --no-install-recommends "$@"; then
    exit 0
  fi
  report "$attempt" install
  sudo -E dpkg --configure -a || true
  sleep 15
done

echo "::error::apt-get could not install: $*"
exit 1
