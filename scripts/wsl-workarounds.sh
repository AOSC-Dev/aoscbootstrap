echo "Creating an empty /etc/machine-id file for WSL ..."
touch /etc/machine-id

echo "Masking tmp.mount and user-runtime-dir@.service for WSLg ..."
systemctl mask tmp.mount user-runtime-dir@.service
