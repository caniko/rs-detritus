/*
  Multi-instance test VM for detritus.

  Declares two independent detritus instances on the same host:
    • instance "acme"  — listens on 0.0.0.0:4317, dataDir /var/lib/detritus-acme
    • instance "beta"  — listens on 0.0.0.0:4318, dataDir /var/lib/detritus-beta

  This VM is exposed as nixosConfigurations.detritus-multi-test-vm in flake.nix.
  It validates that the multi-instance module path evaluates to a buildable
  system toplevel with two separate systemd service units.
*/

{ self }:
{
  config,
  pkgs,
  ...
}:

let
  # Shared test token content (same hash — purely for evaluation; the daemon
  # is not started in a build-only check).
  makeTestTokens =
    project:
    pkgs.writeText "detritus-test-tokens-${project}.toml" ''
      [[token]]
      id = "test-token"
      secret = "$argon2id$v=19$m=65536,t=3,p=4$mmhQFlrCy0uwrddzw5b+GA$LSm5F1I1ZWhZ5xkqYb6p0cQEykz0dvySrWSTAksTGM8"
      project = "${project}"
      source_prefix = "${project}/"

      [rate_limit]
      logs_per_minute = 1000
      logs_burst = 200
      crashes_per_minute = 30
      crashes_burst = 5
    '';

  acmeTokens = makeTestTokens "acme";
  betaTokens = makeTestTokens "beta";
in
{
  imports = [
    self.nixosModules.default
  ];

  system.stateVersion = "26.05";

  boot.loader.grub.devices = [ "nodev" ];
  fileSystems."/" = {
    device = "tmpfs";
    fsType = "tmpfs";
    options = [ "mode=0755" ];
  };

  networking.hostName = "detritus-multi-test-vm";
  networking.firewall.enable = false;

  environment.systemPackages = [
    pkgs.curl
  ];

  # Provision tokens for both instances at build time (read-only store paths,
  # so the preflight ownership check will fail at runtime — that's fine for a
  # build-only evaluation test).
  systemd.tmpfiles.rules = [
    "d /run/detritus-acme 0750 detritus-acme detritus-acme -"
    "C /run/detritus-acme/tokens.toml 0400 detritus-acme detritus-acme - ${acmeTokens}"
    "d /run/detritus-beta 0750 detritus-beta detritus-beta -"
    "C /run/detritus-beta/tokens.toml 0400 detritus-beta detritus-beta - ${betaTokens}"
  ];

  # Multi-instance configuration — uses the new instances attrset.
  # Note: services.detritus.enable is NOT set (that is the shorthand path).
  services.detritus.instances = {
    acme = {
      bind = "0.0.0.0:4317";
      dataDir = "/var/lib/detritus-acme";
      knownDataDirs = [ "/var/lib/detritus-acme" ];
      tokensConfig = "/run/detritus-acme/tokens.toml";
      logsTtlDays = 14;
      crashesTtlDays = 90;
      openFirewall = false;
    };
    beta = {
      bind = "0.0.0.0:4318";
      dataDir = "/var/lib/detritus-beta";
      knownDataDirs = [ "/var/lib/detritus-beta" ];
      tokensConfig = "/run/detritus-beta/tokens.toml";
      logsTtlDays = 7;
      crashesTtlDays = 30;
      openFirewall = false;
    };
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
      {
        from = "host";
        host.port = 4318;
        guest.port = 4318;
      }
    ];
  };
}
