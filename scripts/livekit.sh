echo "Generating a LiveKit initramfs ..."
dracut \
    --add "dmsquash-live livenet" "/live-initramfs.img" \
    $(ls /usr/lib/modules/)

echo "Moving kernel image out ..."
mv -v /boot/vmlinu* /kernel

echo "Enabling auto-login ..."
cat > /etc/systemd/system/getty@tty1.service.d/override.conf << EOF
[Service]
Type=simple
ExecStart=
ExecStart=-/sbin/agetty --autologin root --noclear %I 38400 linux
EOF
