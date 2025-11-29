echo "Resetting APT sources.list ..."
if command -v apt-gen-list > /dev/null; then
	apt-gen-list reset-mirror
else
	cat > /etc/apt/sources.list << EOF
deb https://repo.aosc.io/anthon/debs ${BRANCH:-stable} main
EOF
fi
