{ config, pkgs, lib, ... }: {
  users.users.camellia = {
    isNormalUser = true;
    uid = 1001;
    hashedPassword =
      "$6$rounds=1000$libvirtd$ENoC6piIn3NbaQhtu/eGbqW..m4k/2EL6JtoFLeh/zdrMX5ajj75mQ9H7rdeTNztkm5cJX1X4ho6xvJ7MwR5V/";
    extraGroups = [ "wheel" ];
  };

  security.sudo = { wheelNeedsPassword = false; };

  networking.firewall.enable = false;

  system.stateVersion = "24.05";

  imports = [
    (fetchTarball {
      url =
        "https://github.com/nix-community/nixos-vscode-server/tarball/fc900c16efc6a5ed972fb6be87df018bcf3035bc";
      sha256 = "sha256:1rq8mrlmbzpcbv9ys0x88alw30ks70jlmvnfr2j8v830yy5wvw7h";
    })
  ];

  services.vscode-server.enable = true;
  services.openssh.enable = true;

  nix.settings.experimental-features = [ "nix-command" "flakes" ];

  environment.systemPackages = with pkgs; [
    chezmoi
    ethtool
    fish
    git
    starship
    nil
    neovim
    linuxPackages_latest.perf
    python3
    tcpdump
    htop
  ];

  systemd.network.links = {
    virtual = {
      matchConfig = { Driver = "veth"; };
      linkConfig = { MACAddressPolicy = "none"; };
    };
  };
}
