echo "Flattening glvnd alternatives ..."
cd /etc/alternatives
for i in lib*GL*; do
	if [[ $i = *'+32'* ]]; then
		rm -fv /opt/32/lib/${i/+32/}
		ln -sv glvnd/${i/+32/} /opt/32/lib/${i/+32/}
	else
		rm -fv /usr/lib/$i
		ln -sv glvnd/$i /usr/lib/$i
	fi
done

echo "Fixing up /opt/32/lib/ld-linux.so.2 symlink ..."
ln -sv ../opt /usr/opt
rm -v /lib/ld-linux.so.2
ln -sv ../opt/32/lib/ld-linux.so.2 \
    /lib/ld-linux.so.2

# Note: This breaks build with Ciel (from within nspawn).
echo "Dropping all /dev nodes ..."
rm -rv /dev/*
