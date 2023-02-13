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
	if [[ "$STAGE2" != "1" ]]; then
		echo "Generating mainline release ($i) ..."
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
		echo "Generating mainline release ($i, stage2) ..."
		aoscbootstrap \
                    ${ARCH:-$(dpkg --print-architecture)}-bring-up $i ${REPO:-https://repo.aosc.io/debs} \
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
		    --export-tar os-${ARCH:-$(dpkg --print-architecture)}/$i/aosc-os_${i}-stage2_$(date +%Y%m%d)_${ARCH:-$(dpkg --print-architecture)}.tar.xz \
		    --export-squashfs os-${ARCH:-$(dpkg --print-architecture)}/$i/aosc-os_${i}-stage2_$(date +%Y%m%d)_${ARCH:-$(dpkg --print-architecture)}.squashfs
	fi
    else
	if [[ "$STAGE2" != "1" ]]; then
		echo "Generating Retro release ($i) ..."
	        mkdir -pv os-${ARCH:-$(dpkg --print-architecture)}/${i/retro-/}
	        aoscbootstrap \
	            stable $i ${REPO:-https://repo.aosc.io/debs-retro} \
	            --config /usr/share/aoscbootstrap/config/aosc-retro.toml \
	            -x \
	            --arch ${ARCH:-$(dpkg --print-architecture)} \
	            -s /usr/share/aoscbootstrap/scripts/reset-repo.sh \
	            --include-files /usr/share/aoscbootstrap/recipes/$i.lst \
		    --export-tar os-${ARCH:-$(dpkg --print-architecture)}/${i/retro-/}/aosc-os_${i/retro-/}_$(date +%Y%m%d)_${ARCH:-$(dpkg --print-architecture)}.tar.xz \
		    --export-squashfs os-${ARCH:-$(dpkg --print-architecture)}/${i/retro-/}/aosc-os_${i/retro-/}_$(date +%Y%m%d)_${ARCH:-$(dpkg --print-architecture)}.squashfs
	else
		echo "Generating Retro release ($i, stage2) ..."
		mkdir -pv os-${ARCH:-$(dpkg --print-architecture)}/${i/retro-/}
		aoscbootstrap \
		    ${ARCH:-$(dpkg --print-architecture)}-bring-up $i ${REPO:-https://repo.aosc.io/debs-retro} \
		    --config /usr/share/aoscbootstrap/config/aosc-retro.toml \
		    -x \
		    --arch ${ARCH:-$(dpkg --print-architecture)} \
		    -s /usr/share/aoscbootstrap/scripts/reset-repo.sh \
		    --include-files /usr/share/aoscbootstrap/recipes/$i.lst \
		    --export-tar os-${ARCH:-$(dpkg --print-architecture)}/${i/retro-/}/aosc-os_${i/retro-/}-stage2_$(date +%Y%m%d)_${ARCH:-$(dpkg --print-architecture)}.tar.xz \
		    --export-squashfs os-${ARCH:-$(dpkg --print-architecture)}/${i/retro-/}/aosc-os_${i/retro-/}-stage2_$(date +%Y%m%d)_${ARCH:-$(dpkg --print-architecture)}.squashfs
	fi
    fi
    rm -r $i

    # Hack, just to make sure that things are catching up (we observed a weird caching issue on kp920).
    sync
    sleep 1
done
