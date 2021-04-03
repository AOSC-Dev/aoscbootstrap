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
    DPKG_FILES="$(mktemp)"
    ALL_FILES="$(mktemp)"
    RM_FILES="$(mktemp)"
    PATTERN_FILES="$(mktemp)"
    echo 'Listing dpkg packages ...'
    PACKAGES="$(dpkg-query --show --showformat="\${Package}\n")"
    echo 'Collecting files from dpkg ...'
    find / -mindepth 2 >> "$ALL_FILES"
    for p in $PACKAGES; do
        dpkg-query --listfiles "$p" >> "$DPKG_FILES"
    done
    echo "$WHITELIST" > "$PATTERN_FILES"
    grep -vEf "$PATTERN_FILES" < "$ALL_FILES" > "${ALL_FILES}.new"
    mv "${ALL_FILES}.new" "$ALL_FILES"
    grep -vxFf "$DPKG_FILES" < "$ALL_FILES" > "$RM_FILES"
    echo 'Removing files ...'
    xargs -L 1000 -a "$RM_FILES" rm -rfv
    rm -fv "$ALL_FILES" "$DPKG_FILES" "$RM_FILES"
}

set -eo pipefail
echo 'Cleaning up the installation ...'
remove_files
