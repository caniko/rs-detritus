{self}: {
  config,
  pkgs,
  ...
}: let
  testTokens = pkgs.writeText "detritus-test-tokens.toml" ''
    [[token]]
    id = "test-token"
    secret = "$argon2id$v=19$m=65536,t=3,p=4$mmhQFlrCy0uwrddzw5b+GA$LSm5F1I1ZWhZ5xkqYb6p0cQEykz0dvySrWSTAksTGM8"
    project = "detritus"
    source_prefix = "detritus/"

    [rate_limit]
    logs_per_minute = 1000
    logs_burst = 200
    crashes_per_minute = 30
    crashes_burst = 5
  '';
in {
  imports = [
    self.nixosModules.default
  ];

  system.stateVersion = "26.05";

  boot.loader.grub.devices = ["nodev"];
  fileSystems."/" = {
    device = "tmpfs";
    fsType = "tmpfs";
    options = ["mode=0755"];
  };

  networking.hostName = "detritus-test-vm";
  networking.firewall.enable = false;

  environment.systemPackages = [
    pkgs.curl
  ];

  systemd.tmpfiles.rules = [
    "d /run/detritus 0750 detritus detritus -"
    "C /run/detritus/tokens.toml 0400 detritus detritus - ${testTokens}"
  ];

  services.detritus = {
    enable = true;
    bind = "0.0.0.0:4317";
    tokensConfig = "/run/detritus/tokens.toml";
    logsTtlDays = 14;
    crashesTtlDays = 90;
  };

  virtualisation.vmVariant = {
    virtualisation.memorySize = 1024;
    virtualisation.cores = 1;
    virtualisation.graphics = false;
    virtualisation.forwardPorts = [
      {
        from = "host";
        host.port = 4317;
        guest.port = 4317;
      }
    ];
  };
}
