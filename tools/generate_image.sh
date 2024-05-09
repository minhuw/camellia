#!/bin/env bash

[ -f "result" ] && rm result
[ -f /opt/camellia/nixos.qcow2 ] && sudo rm /opt/camellia/nixos.qcow2

nix build .#dev-image

sudo cp ./result/nixos.qcow2 /opt/camellia/nixos.qcow2
sudo chown -R libvirt-qemu:kvm /opt/camellia/
sudo chmod 644 /opt/camellia/nixos.qcow2

