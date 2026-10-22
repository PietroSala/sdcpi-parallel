#!/bin/sh
# Docker and the artifact image in the TACAS 2027 virtual machine, with no
# network: the Docker of Ubuntu 26.04 is installed from the packages of the
# archive that the record carries, and the image of the architecture of the
# machine is loaded. Run it as root from the folder that holds the files of
# the record, for instance the shared folder of VirtualBox:
#
#     cd /media/sf_artifact && sudo sh vm-setup.sh
#
# The packages are exactly those that apt installs for docker.io on the clean
# machine: docker.io 29.1.3, containerd 2.2.2, runc 1.4.0, pigz, bridge-utils
# and ubuntu-fan, in sdcpi-tacas27-docker-<arch>.tar, each with the SHA-256
# that the index of the archive gives it. A machine with network may run
# `sudo apt-get install docker.io` instead; this script then only loads the
# image. Any failure stops the script.
set -e
cd "$(dirname "$0")"
[ "$(id -u)" = 0 ] || { echo "run it as root: sudo sh $0"; exit 1; }
ARCH=$(dpkg --print-architecture)
DEBS=sdcpi-tacas27-docker-$ARCH.tar
IMAGE=sdcpi-tacas27-image-$ARCH.tar.gz
[ -f "$IMAGE" ] || { echo "missing $IMAGE in $(pwd)"; exit 1; }

echo "== 1. Docker"
if command -v docker >/dev/null 2>&1; then
    echo "already installed"
else
    [ -f "$DEBS" ] || { echo "missing $DEBS in $(pwd)"; exit 1; }
    TMP=$(mktemp -d)
    tar -xf "$DEBS" -C "$TMP"
    dpkg -i "$TMP"/*.deb
    rm -rf "$TMP"
fi
systemctl start docker
docker version --format 'Docker {{.Server.Version}}'

echo "== 2. The artifact image, for $ARCH"
docker load -i "$IMAGE"
docker image ls sdcpi-tacas27
echo "SETUP DONE"
