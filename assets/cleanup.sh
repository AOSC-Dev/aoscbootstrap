#!/bin/bash -e
# === cleanup.sh
remove_files () {
WHITELIST="^/dev
^/efi
^/etc
^/run
^/usr
^/var/lib/apt/gen
^/var/lib/apt/extended_states
^/var/lib/dkms
^/var/lib/dpkg
^/var/log/journal$
^/usr/lib/locale/locale-archive
^/root
^/home
^/proc
^/sys
/\.updated$"
    local DPKG_FILES ALL_FILES RM_FILES PATTERN_FILES
    local FIND_PID
    DPKG_FILES="$(mktemp)"
    ALL_FILES="$(mktemp)"
    RM_FILES="$(mktemp)"
    PATTERN_FILES="$(mktemp)"
    echo -e '\e[1mCollecting files from dpkg ...\e[0m'
    # Accessing /proc causes race conditions. Better to avoid accessing pseudo filesystems.
    # Note: using `-not -regex' does not prevent find from accessing them.
    # `-prune` is more flexible, but `-mount` would be better.
    find / -mindepth 2 \( -path '/sys/*' -o -path '/proc/*' -o -path '/dev/*' -o -path '/tmp/*' -o -path '/run/*' \) -prune -o -print >> "$ALL_FILES" &
    FIND_PID="$!"
    cat /var/lib/dpkg/info/*.list > "$DPKG_FILES"
    wait "$FIND_PID"
    echo "$WHITELIST" > "$PATTERN_FILES"
    grep -vEf "$PATTERN_FILES" < "$ALL_FILES" > "${ALL_FILES}.new"
    mv "${ALL_FILES}.new" "$ALL_FILES"
    grep -vxFf "$DPKG_FILES" < "$ALL_FILES" > "$RM_FILES"
    echo -e '\e[1mRemoving files ...\e[0m'
    xargs -a "$RM_FILES" rm -rfv
    rm -fv "$ALL_FILES" "$DPKG_FILES" "$RM_FILES"
    # Remove some extra files that absolutely should not be in the release files.
    echo -e '\e[1mRemoving sensitive files ...\e[0m'
    rm -fv /etc/machine-id
}

set -eo pipefail
echo -e '\e[1mCleaning up the installation ...\e[0m'
remove_files
