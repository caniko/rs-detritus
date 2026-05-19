{ self }:
{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.services.detritus;
  defaultDataDir = "/var/lib/detritus";
  bindPort = builtins.fromJSON (lib.last (lib.splitString ":" cfg.bind));
  packageDefault = self.packages.${pkgs.stdenv.hostPlatform.system}.detritus;
  tokenPreflight = pkgs.writeShellScript "detritus-token-preflight" ''
    set -eu

    tokens=${lib.escapeShellArg (toString cfg.tokensConfig)}
    expected=${lib.escapeShellArg "400:${cfg.user}:${cfg.group}"}
    actual="$(${pkgs.coreutils}/bin/stat -Lc '%a:%U:%G' "$tokens")"

    if [ "$actual" != "$expected" ]; then
      echo "detritus tokensConfig must be mode 0400 owned by ${cfg.user}:${cfg.group}; got $actual" >&2
      exit 1
    fi
  '';
in
{
  options.services.detritus = {
    enable = lib.mkEnableOption "detritus log and crash dump receiver";

    package = lib.mkOption {
      type = lib.types.package;
      default = packageDefault;
      defaultText = lib.literalExpression "self.packages.\${pkgs.stdenv.hostPlatform.system}.detritus";
      description = "detritus package to run.";
    };

    bind = lib.mkOption {
      type = lib.types.str;
      default = "127.0.0.1:4317";
      description = "Socket address detritusd listens on.";
    };

    dataDir = lib.mkOption {
      type = lib.types.path;
      default = defaultDataDir;
      description = ''
        Persistent storage root for logs, crash blobs, indexes, and temporary upload files.

        Changing this after deployment does not migrate existing data; it leaves old blobs and
        indexes in the previous path. Add intentional historical paths to knownDataDirs before
        changing this option.
      '';
    };

    knownDataDirs = lib.mkOption {
      type = lib.types.listOf lib.types.path;
      default = [ defaultDataDir ];
      description = "Approved persistent data directories. This guards against accidental dataDir changes.";
    };

    tokensConfig = lib.mkOption {
      type = lib.types.path;
      description = "TOML file containing Argon2-hashed bearer-token entries.";
    };

    logsTtlDays = lib.mkOption {
      type = lib.types.ints.positive;
      default = 14;
      description = "Number of days to retain log NDJSON files.";
    };

    crashesTtlDays = lib.mkOption {
      type = lib.types.ints.positive;
      default = 90;
      description = "Number of days to retain crash source indexes and referenced blobs.";
    };

    user = lib.mkOption {
      type = lib.types.str;
      default = "detritus";
      description = "User account used to run detritusd.";
    };

    group = lib.mkOption {
      type = lib.types.str;
      default = "detritus";
      description = "Group used to run detritusd.";
    };

    openFirewall = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Open the detritus TCP port in the host firewall. Leave disabled behind a reverse proxy.";
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = builtins.elem (toString cfg.dataDir) (map toString cfg.knownDataDirs);
        message = ''
          services.detritus.dataDir is ${toString cfg.dataDir}, which is not in
          services.detritus.knownDataDirs. This guard prevents accidental production
          data orphaning. Add the intentional path to knownDataDirs if this move is deliberate.
        '';
      }
    ];

    users.groups.${cfg.group} = { };
    users.users.${cfg.user} = {
      isSystemUser = true;
      group = cfg.group;
    };

    systemd.tmpfiles.rules = [
      "d ${toString cfg.dataDir} 0750 ${cfg.user} ${cfg.group} -"
    ];

    systemd.services.detritus = {
      description = "detritus log and crash dump receiver";
      wantedBy = [ "multi-user.target" ];
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];

      serviceConfig = {
        Type = "simple";
        ExecStartPre = "+${tokenPreflight}";
        ExecStart = "${cfg.package}/bin/detritusd ${lib.escapeShellArgs [
          "--bind"
          cfg.bind
          "--data-dir"
          (toString cfg.dataDir)
          "--tokens-config"
          (toString cfg.tokensConfig)
          "--logs-ttl-days"
          (toString cfg.logsTtlDays)
          "--crashes-ttl-days"
          (toString cfg.crashesTtlDays)
          "--log-format"
          "json"
        ]}";
        Restart = "on-failure";
        RestartSec = "10s";
        User = cfg.user;
        Group = cfg.group;
        StateDirectory = lib.mkIf (toString cfg.dataDir == defaultDataDir) "detritus";
        StateDirectoryMode = "0750";
        ReadWritePaths = [ (toString cfg.dataDir) ];
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        PrivateDevices = true;
        NoNewPrivileges = true;
        RestrictAddressFamilies = [
          "AF_INET"
          "AF_INET6"
          "AF_UNIX"
        ];
        RestrictNamespaces = true;
        RestrictRealtime = true;
        LockPersonality = true;
        MemoryDenyWriteExecute = true;
        ProtectClock = true;
        ProtectControlGroups = true;
        ProtectKernelLogs = true;
        ProtectKernelModules = true;
        ProtectKernelTunables = true;
        SystemCallArchitectures = "native";
        CapabilityBoundingSet = "";
      };
    };

    networking.firewall.allowedTCPPorts = lib.mkIf cfg.openFirewall [ bindPort ];
  };
}
