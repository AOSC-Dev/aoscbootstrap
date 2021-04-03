#!/bin/bash
# === bootstrap.sh
set -eo pipefail
echo 'Setting up password ...'
echo 'root:anthon' | chpasswd
count=0
PACKAGES=(
{}
)
length=${#PACKAGES[@]}
for p in "${PACKAGES[@]}"; do
count=$((count+1))
echo "[$count/$length] Installing ${p}..."
dpkg --force-depends --unpack "/var/cache/apt/archives/${p}"
done
count_c=1;length_c=$(dpkg -l | grep -c 'iU')
function dpkg_progress () {
    while read action step package; do
if [ "$action" = 'processing:' ] && [ "$step" = 'configure:' ]; then
echo "[$count_c/$length_c] Configuring $package...";count_c=$(( count_c + 1 ))
fi
    done
}
{ dpkg --status-fd=7 --configure --pending --force-configure-any --force-depends 7>&1 >&8 | dpkg_progress; } 8>&1 \
|| { echo 'Configuring missed packages ...'; dpkg --configure -a; }
echo 'Copying skeleton files ...'
cp -rvT /etc/skel /root
