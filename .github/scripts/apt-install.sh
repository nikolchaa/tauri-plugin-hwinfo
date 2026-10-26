#!/usr/bin/env bash
# Installs apt packages on a GitHub-hosted Ubuntu runner without letting one
# bad moment hang the job. Bare `apt-get` has stalled runs into their job
# limits, and the step log needs a sign-in to read, so every stage is bounded
# and a failed attempt reports what it was stuck on as a public annotation:
#   - a mirror that stops sending:       HTTP timeouts + apt's own retries
#   - a mirror that crawls:              a hard limit per attempt, then a retry
#                                        that bypasses the Azure mirror; files
#                                        already downloaded are kept
#   - a dpkg killed mid-install:         `dpkg --configure -a` before retrying
#   - another apt process holding locks: wait for it, but not forever
#   - man-db reindexing after install:   disabled; it can take minutes alone
#
# The Azure mirror has served packages at ~40 KB/s while handing out indexes at
# 10 MB/s, which no timeout tells apart from progress. Its failover to
# archive.ubuntu.com only fires on errors, so a retry removes it outright.
#
# With APT_ARCHIVES set, downloaded packages live in that directory instead of
# /var/cache/apt/archives, readable by the runner user so `actions/cache` can
# keep them between runs.
#
# Usage: [APT_ARCHIVES=<dir>] bash .github/scripts/apt-install.sh <package>...
set -euo pipefail

export DEBIAN_FRONTEND=noninteractive

sudo rm -f /var/lib/man-db/auto-update
cat <<'EOF' | sudo tee /etc/apt/apt.conf.d/80-ci > /dev/null
Acquire::Retries "3";
Acquire::http::Timeout "30";
Acquire::https::Timeout "30";
DPkg::Lock::Timeout "120";
EOF

archive_opts=()
if [ -n "${APT_ARCHIVES:-}" ]; then
  mkdir -p "${APT_ARCHIVES}/partial"
  archive_opts=(-o "Dir::Cache::Archives=${APT_ARCHIVES}")
fi

log="$(mktemp)"

# One apt-get command under a hard time limit, its output kept for `report`.
apt() {
  local limit=$1
  shift
  sudo -E timeout --kill-after=15 "$limit" apt-get "${archive_opts[@]}" "$@" 2>&1 | tee "$log"
}

report() {
  local tail
  tail="$(tail -n 4 "$log" | tr '\n' ' ' | cut -c1-400)"
  echo "::warning::apt attempt $1 of 3 failed during '$2': ${tail}"
}

# The image lists mirrors in /etc/apt/apt-mirrors.txt; older images name the
# Azure mirror in the sources directly. Either way the indexes are refetched.
skip_azure_mirror() {
  if [ -f /etc/apt/apt-mirrors.txt ]; then
    sudo sed -i '/azure\.archive\.ubuntu\.com/d' /etc/apt/apt-mirrors.txt
  fi
  sudo find /etc/apt -name '*.sources' -o -name '*.list' |
    xargs -r sudo sed -i 's#http://azure\.archive\.ubuntu\.com/ubuntu#http://archive.ubuntu.com/ubuntu#g'
  echo "::notice::apt now bypasses azure.archive.ubuntu.com"
}

# Keeps the cache to what this package set needs, and readable for
# `actions/cache`, which runs as the runner user.
tidy_archives() {
  [ -n "${APT_ARCHIVES:-}" ] || return 0
  sudo -E apt-get "${archive_opts[@]}" autoclean -q > /dev/null || true
  sudo rm -rf "${APT_ARCHIVES}/partial" "${APT_ARCHIVES}/lock"
  sudo chmod -R a+rX "${APT_ARCHIVES}"
}

# Recommends would pull in docs, fonts and desktop extras the build never
# touches; WebKit's tree is several hundred megabytes of them.
indexed=false
for attempt in 1 2 3; do
  if [ "$attempt" -gt 1 ]; then
    skip_azure_mirror
    indexed=false
  fi
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
    tidy_archives
    exit 0
  fi
  report "$attempt" install
  sudo -E dpkg --configure -a || true
  sleep 15
done

echo "::error::apt-get could not install: $*"
exit 1
