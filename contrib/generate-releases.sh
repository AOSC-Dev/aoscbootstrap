#!/bin/bash

_help_message() {
    printf "\
Usage:

        generate-releases.sh VARIANTS

	        - VARIANTS: A list of variants to generate
                  (e.g. cinnamon gnome kde lxde mate xfce).

	Optional variables:

		- ARCH: Define tarball architecture
                  (falls back to dpkg architecture).
                - REPO: Repository mirror to use.

"
}

if [[ $# -eq 0 || "$1" == "--help" || "$1" == "-h" ]]; then
    _help_message
    exit 0
fi

export TZ='UTC'

mkdir -pv os-${ARCH:-$(dpkg --print-architecture)}

for i in $@; do
    if [[ "$RETRO" != "1" ]]; then
        mkdir -pv os-${ARCH:-$(dpkg --print-architecture)}/$i
        aoscbootstrap \
	    stable $i ${REPO:-https://repo.aosc.io/debs} \
            --config /usr/share/aoscbootstrap/config/aosc-mainline.toml \
            -x \
            --arch ${ARCH:-$(dpkg --print-architecture)} \
            -s \
                /usr/share/aoscbootstrap/scripts/reset-repo.sh \
	    -s \
                /usr/share/aoscbootstrap/scripts/enable-nvidia-drivers.sh \
	    -s \
                /usr/share/aoscbootstrap/scripts/enable-dkms.sh \
            --include-files /usr/share/aoscbootstrap/recipes/$i.lst \
	    --export-tar os-${ARCH:-$(dpkg --print-architecture)}/$i/aosc-os_${i}_$(date +%Y%m%d)_${ARCH:-$(dpkg --print-architecture)}.tar.xz \
	    --export-squashfs os-${ARCH:-$(dpkg --print-architecture)}/$i/aosc-os_${i}_$(date +%Y%m%d)_${ARCH:-$(dpkg --print-architecture)}.squashfs
    else
	echo "Generating for Retro ..."
        mkdir -pv os-${ARCH:-$(dpkg --print-architecture)}/$i
        aoscbootstrap \
            stable $i ${REPO:-https://repo.aosc.io/debs-retro} \
            --config /usr/share/aoscbootstrap/config/aosc-retro.toml \
            -x \
            --arch ${ARCH:-$(dpkg --print-architecture)} \
            -s /usr/share/aoscbootstrap/scripts/reset-repo.sh \
            --include-files /usr/share/aoscbootstrap/recipes/$i.lst \
	    --export-tar os-${ARCH:-$(dpkg --print-architecture)}/$i/aosc-os_${i}_$(date +%Y%m%d)_${ARCH:-$(dpkg --print-architecture)}.tar.xz \
	    --export-squashfs os-${ARCH:-$(dpkg --print-architecture)}/$i/aosc-os_${i}_$(date +%Y%m%d)_${ARCH:-$(dpkg --print-architecture)}.squashfs
    fi
    rm -r $i

    # Hack, just to make sure that things are catching up (we observed a weird caching issue on kp920).
    sync
    sleep 1
done
