echo "Creating a default user ..."
useradd aosc -m
usermod -a -G audio,cdrom,video,wheel aosc

echo "Setting default password ..."
echo 'aosc:anthon' | chpasswd -c SHA512 -R /

echo "Enabling RootFS adjustment service ..."
touch /.rootfs-repartition
systemctl enable rootfs-grow.service
