#!/bin/bash
# === bootstrap.sh
set -eo pipefail
count=0
PACKAGES=(
{}
)
length=${#PACKAGES[@]}
for p in "${PACKAGES[@]}"; do
count=$((count+1))
echo -e "\e[1m[$count/$length] Installing ${p}...\e[0m"
dpkg --force-depends --unpack "/var/cache/apt/archives/${p}"
done
count_c=1;length_c=$(dpkg -l | grep -c 'iU')
function dpkg_progress () {
    while read action step package; do
if [ "$action" = 'processing:' ] && [ "$step" = 'configure:' ]; then
echo -e "\e[1m\e[96m[$count_c/$length_c] Configuring $package...\e[0m";count_c=$(( count_c + 1 ))
fi
    done
}
{ DEBIAN_FRONTEND=noninteractive dpkg --status-fd=7 --configure --pending --force-configure-any --force-depends 7>&1 >&8 | dpkg_progress; } 8>&1 \
|| { echo 'Configuring missed packages ...'; dpkg --configure -a; }
echo -e '\e[1m\e[94mCopying skeleton files ...\e[0m'
cp -rvT /etc/skel /root
echo -e '\e[1m\e[94mEnabling systemd vendor presets ...\e[0m'
systemctl preset-all
