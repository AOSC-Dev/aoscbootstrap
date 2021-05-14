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
    echo -e '\e[1mListing dpkg packages ...\e[0m'
    mapfile -t PACKAGES < <(dpkg-query --show --showformat="\${Package}\n")
    echo -e '\e[1mCollecting files from dpkg ...\e[0m'
    find / -mindepth 2 >> "$ALL_FILES"
    dpkg-query --listfiles "${PACKAGES[@]}" > "$DPKG_FILES"
    echo -e "$WHITELIST" > "$PATTERN_FILES"
    grep -vEf "$PATTERN_FILES" < "$ALL_FILES" > "${ALL_FILES}.new"
    mv "${ALL_FILES}.new" "$ALL_FILES"
    grep -vxFf "$DPKG_FILES" < "$ALL_FILES" > "$RM_FILES"
    echo -e '\e[1mRemoving files ...\e[0m'
    xargs -L 1000 -a "$RM_FILES" rm -rfv
    rm -fv "$ALL_FILES" "$DPKG_FILES" "$RM_FILES"
}

set -eo pipefail
echo -e '\e[1mCleaning up the installation ...\e[0m'
remove_files
