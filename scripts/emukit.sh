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
