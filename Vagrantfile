# -*- mode: ruby -*-
# vi: set ft=ruby :

Vagrant.configure("2") do |config|
  config.vm.box = "cloud-image/ubuntu-24.04"
  config.vm.hostname = "camellia-net-dev"

  config.vm.provider "virtualbox" do |vb|
    vb.memory = "4096"
    vb.cpus = 4
  end

  config.vm.provider "libvirt" do |lv|
    lv.memory = 4096
    lv.cpus = 4
  end

  config.vm.synced_folder ".", "/home/vagrant/camellia-net"

  config.vm.provision "shell", inline: <<-SHELL
    set -eux

    export DEBIAN_FRONTEND=noninteractive

    apt-get update
    apt-get install -y \
      curl \
      build-essential \
      gcc-multilib \
      zlib1g-dev \
      libelf-dev \
      libpcap-dev \
      m4 \
      libclang-dev \
      llvm \
      clang \
      pkg-config \
      ethtool \
      iputils-ping \
      iperf \
      iperf3 \
      tcpdump

    # Install Rust for the vagrant user
    su - vagrant -c 'curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y'

    # Set ulimits for memlock and nofile (needed for XDP/AF_XDP)
    cat > /etc/security/limits.d/camellia-net.conf <<EOF
*  soft  memlock  unlimited
*  hard  memlock  unlimited
*  soft  nofile   1048576
*  hard  nofile   1048576
EOF

    # Also apply via sysctl for current session
    sysctl -w vm.max_map_count=1048576

    # Allow unprivileged users to use BPF (needed for some XDP operations)
    sysctl -w kernel.unprivileged_bpf_disabled=0

    echo "Provisioning complete. Run: vagrant ssh"
    echo "Then: cd camellia-net && cargo build"
    echo "Tests: cd camellia-net && sudo -E env PATH=\\$PATH cargo test"
  SHELL
end
