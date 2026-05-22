/*
detritus NixOS module — two deployment modes
=============================================

## Single-instance shorthand (existing API, unchanged)

  services.detritus = {
    enable = true;
    bind         = "127.0.0.1:4317";
    tokensConfig = "/run/detritus/tokens.toml";
    # dataDir, logsTtlDays, crashesTtlDays, user, group, openFirewall…
  };

This produces a single systemd unit named `detritus.service`, owned by
the `detritus` user/group, exactly as before the multi-instance refactor.
DO NOT set any `services.detritus.instances.*` key at the same time —
the module raises an assertion failure if you mix both forms.

## Multi-instance attrset (new API)

  services.detritus.instances = {
    "acme" = {
      bind         = "0.0.0.0:4317";
      dataDir      = "/var/lib/detritus-acme";
      tokensConfig = "/run/secrets/detritus-acme.toml";
      user         = "detritus-acme";   # default: "detritus-<name>"
      group        = "detritus-acme";   # default: "detritus-<name>"
      openFirewall = true;
    };
    "beta" = {
      bind         = "0.0.0.0:4318";
      dataDir      = "/var/lib/detritus-beta";
      tokensConfig = "/run/secrets/detritus-beta.toml";
    };
  };

Each instance produces:
  • systemd.services."detritus-<name>"
  • users.users."<user>"  + users.groups."<group>"
  • A tokens-preflight script baked with its own user/group
  • A firewall port opening when openFirewall = true
  • BindReadOnlyPaths for schemaDir (when non-null)

## Per-instance option defaults

  bind            = "127.0.0.1:4317"
  dataDir         = "/var/lib/detritus-<name>"
  knownDataDirs   = [ dataDir ]
  logsTtlDays     = 14
  crashesTtlDays  = 90
  user            = "detritus-<name>"
  group           = "detritus-<name>"
  openFirewall    = false
  schemaDir       = null

## User/group naming convention

Explicit instances use "detritus-<name>" by default to avoid colliding
with the historic "detritus" system user created by the single-instance
shorthand. If you want instances to share a system user, set `user` and
`group` explicitly on both.

## NOTE on systemd unit names

Single-instance shorthand → `detritus.service`  (backward-compatible)
Multi-instance attrset    → `detritus-<name>.service`  (new)
*/
{self}: {
  config,
  lib,
  pkgs,
  ...
}: let
  cfg = config.services.detritus;

  # ---------------------------------------------------------------------------
  # Helpers
  # ---------------------------------------------------------------------------

  # Default data directory for an explicit instance (NOT the legacy "default").
  instanceDefaultDataDir = name: "/var/lib/detritus-${name}";

  # Parse the port from a "host:port" bind string.
  bindPortOf = bind: lib.toInt (lib.last (lib.splitString ":" bind));

  # Build one tokens-preflight shell script for a given instance config.
  # scriptSuffix: appended to "detritus-token-preflight" (pass "" for the
  # legacy single-instance case to preserve the historic store-path name).
  makeTokenPreflight = scriptSuffix: instanceCfg:
    pkgs.writeShellScript "detritus-token-preflight${scriptSuffix}" ''
      set -eu

      tokens=${lib.escapeShellArg (toString instanceCfg.tokensConfig)}
      expected=${lib.escapeShellArg "400:${instanceCfg.user}:${instanceCfg.group}"}
      actual="$(${pkgs.coreutils}/bin/stat -Lc '%a:%U:%G' "$tokens")"

      if [ "$actual" != "$expected" ]; then
        echo "detritus tokensConfig must be mode 0400 owned by ${instanceCfg.user}:${instanceCfg.group}; got $actual" >&2
        exit 1
      fi
    '';

  # Build one systemd service attrset for a given instance config.
  # unitName: the key under systemd.services (e.g. "detritus" or "detritus-acme").
  # isLegacyDefault: true when synthesised from the single-instance shorthand —
  #   controls StateDirectory (use "detritus", not "detritus-default").
  makeServiceConfig = unitName: isLegacyDefault: instanceCfg: let
    # Use empty suffix for the legacy default to keep the historic store-path name.
    scriptSuffix =
      if isLegacyDefault
      then ""
      else "-${instanceCfg.name}";
    tokenPreflight = makeTokenPreflight scriptSuffix instanceCfg;
    legacyDefaultDataDir = "/var/lib/detritus";
    useStateDir =
      if isLegacyDefault
      then toString instanceCfg.dataDir == legacyDefaultDataDir
      else toString instanceCfg.dataDir == instanceDefaultDataDir instanceCfg.name;
    stateDirectoryName =
      if isLegacyDefault
      then "detritus"
      else "detritus-${instanceCfg.name}";
  in {
    description = "detritus log and crash dump receiver [${instanceCfg.name}]";
    wantedBy = ["multi-user.target"];
    after = ["network-online.target"];
    wants = ["network-online.target"];

    serviceConfig =
      {
        Type = "simple";
        ExecStartPre = "+${tokenPreflight}";
        ExecStart = "${instanceCfg.package}/bin/detritusd ${lib.escapeShellArgs [
          "--bind"
          instanceCfg.bind
          "--data-dir"
          (toString instanceCfg.dataDir)
          "--tokens-config"
          (toString instanceCfg.tokensConfig)
          "--logs-ttl-days"
          (toString instanceCfg.logsTtlDays)
          "--crashes-ttl-days"
          (toString instanceCfg.crashesTtlDays)
          "--log-format"
          "json"
        ]}";
        Restart = "on-failure";
        RestartSec = "10s";
        User = instanceCfg.user;
        Group = instanceCfg.group;
        StateDirectoryMode = "0750";
        ReadWritePaths = [(toString instanceCfg.dataDir)] ++ lib.optional (instanceCfg.schemaDir != null) (toString instanceCfg.schemaDir);
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
      }
      // lib.optionalAttrs useStateDir {StateDirectory = stateDirectoryName;}
      // lib.optionalAttrs (instanceCfg.schemaDir != null) {
        BindReadOnlyPaths = [(toString instanceCfg.schemaDir)];
      };
  };

  # ---------------------------------------------------------------------------
  # Instance submodule
  # ---------------------------------------------------------------------------

  instanceSubmodule = {name, ...}: {
    options = {
      # Expose the instance name for use in makeTokenPreflight / makeServiceConfig.
      name = lib.mkOption {
        type = lib.types.str;
        default = name;
        internal = true;
        description = "Instance name (derived from the attribute key).";
      };

      package = lib.mkOption {
        type = lib.types.package;
        default = self.packages.${pkgs.stdenv.hostPlatform.system}.detritus;
        defaultText = lib.literalExpression "self.packages.\${pkgs.stdenv.hostPlatform.system}.detritus";
        description = "detritus package to run for this instance.";
      };

      bind = lib.mkOption {
        type = lib.types.str;
        default = "127.0.0.1:4317";
        description = "Socket address detritusd listens on.";
      };

      dataDir = lib.mkOption {
        type = lib.types.path;
        default = instanceDefaultDataDir name;
        description = ''
          Persistent storage root for logs, crash blobs, indexes, and
          temporary upload files.

          Changing this after deployment does not migrate existing data.
          Add intentional historical paths to knownDataDirs before changing
          this option.
        '';
      };

      knownDataDirs = lib.mkOption {
        type = lib.types.listOf lib.types.path;
        default = [(instanceDefaultDataDir name)];
        description = "Approved persistent data directories for this instance.";
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
        default = "detritus-${name}";
        description = "System user account used to run this detritusd instance.";
      };

      group = lib.mkOption {
        type = lib.types.str;
        default = "detritus-${name}";
        description = "System group used to run this detritusd instance.";
      };

      openFirewall = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = "Open this instance's TCP port in the host firewall.";
      };

      schemaDir = lib.mkOption {
        type = lib.types.nullOr lib.types.path;
        default = null;
        description = ''
          Optional directory of JSON Schema files referenced by this
          instance's tokensConfig [[schema]] entries. When non-null, the
          directory is bind-mounted read-only into the service.
        '';
      };
    };
  };

  # ---------------------------------------------------------------------------
  # Derived values: figure out which instances are actually active.
  # ---------------------------------------------------------------------------

  # True when the user set at least one `services.detritus.instances.*` key.
  hasExplicitInstances = cfg.instances != {};

  # True when the user set the single-instance shorthand (enable = true without instances).
  isShorthand = cfg.enable && !hasExplicitInstances;

  # The set of instances to materialise — either the explicit set or (for the
  # shorthand) a synthesised "default" entry that mirrors the top-level options.
  #
  # IMPORTANT: The synthesised "default" entry uses user = "detritus" and
  # group = "detritus" (NOT "detritus-default") so that existing hosts that
  # already have the "detritus" system user/group don't break on upgrade (F4).
  activeInstances =
    if hasExplicitInstances
    then cfg.instances
    else {
      default = {
        name = "default";
        package = cfg.package;
        bind = cfg.bind;
        dataDir = cfg.dataDir;
        knownDataDirs = cfg.knownDataDirs;
        tokensConfig = cfg.tokensConfig;
        logsTtlDays = cfg.logsTtlDays;
        crashesTtlDays = cfg.crashesTtlDays;
        user = cfg.user;
        group = cfg.group;
        openFirewall = cfg.openFirewall;
        schemaDir = null;
      };
    };

  # For each instance, collect the TCP port (as an int) for firewall rules.
  activePorts = lib.concatMap (
    inst: lib.optional inst.openFirewall (bindPortOf inst.bind)
  ) (lib.attrValues activeInstances);

  # Build the systemd services attrset.
  # The shorthand "default" instance uses the historic unit name "detritus"
  # (not "detritus-default") for backward-compatibility.
  serviceAttrset =
    lib.mapAttrs' (
      name: inst: let
        unitName =
          if (name == "default" && isShorthand)
          then "detritus"
          else "detritus-${name}";
      in
        lib.nameValuePair unitName (makeServiceConfig unitName (name == "default" && isShorthand) inst)
    )
    activeInstances;

  # Build systemd.tmpfiles rules for each instance.
  tmpfilesRules = lib.concatMap (
    inst: ["d ${toString inst.dataDir} 0750 ${inst.user} ${inst.group} -"]
  ) (lib.attrValues activeInstances);

  # Collect all unique (user, group) pairs for users.users / users.groups.
  instanceUsers = lib.attrValues activeInstances;

  # Collect unique groups (by name) across instances.
  uniqueGroups = lib.unique (map (i: i.group) instanceUsers);

  # Build users.users entries for every instance (deduplicated by user name).
  usersAttrset = lib.listToAttrs (
    lib.unique (
      map (inst:
        lib.nameValuePair inst.user {
          isSystemUser = true;
          group = inst.group;
        })
      instanceUsers
    )
  );

  # Build users.groups entries for every unique group.
  groupsAttrset = lib.listToAttrs (map (g: lib.nameValuePair g {}) uniqueGroups);
in {
  options.services.detritus = {
    # -------------------------------------------------------------------------
    # Single-instance shorthand (top-level options) — keep all existing options
    # -------------------------------------------------------------------------

    enable = lib.mkEnableOption "detritus log and crash dump receiver";

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.detritus;
      defaultText = lib.literalExpression "self.packages.\${pkgs.stdenv.hostPlatform.system}.detritus";
      description = "detritus package to run (single-instance shorthand).";
    };

    bind = lib.mkOption {
      type = lib.types.str;
      default = "127.0.0.1:4317";
      description = "Socket address detritusd listens on (single-instance shorthand).";
    };

    dataDir = lib.mkOption {
      type = lib.types.path;
      default = "/var/lib/detritus";
      description = ''
        Persistent storage root for logs, crash blobs, indexes, and temporary upload files.

        Changing this after deployment does not migrate existing data; it leaves old blobs and
        indexes in the previous path. Add intentional historical paths to knownDataDirs before
        changing this option.

        (Single-instance shorthand — use services.detritus.instances for multi-instance.)
      '';
    };

    knownDataDirs = lib.mkOption {
      type = lib.types.listOf lib.types.path;
      default = ["/var/lib/detritus"];
      description = "Approved persistent data directories. This guards against accidental dataDir changes. (Single-instance shorthand.)";
    };

    tokensConfig = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      description = "TOML file containing Argon2-hashed bearer-token entries. (Single-instance shorthand.)";
    };

    logsTtlDays = lib.mkOption {
      type = lib.types.ints.positive;
      default = 14;
      description = "Number of days to retain log NDJSON files. (Single-instance shorthand.)";
    };

    crashesTtlDays = lib.mkOption {
      type = lib.types.ints.positive;
      default = 90;
      description = "Number of days to retain crash source indexes and referenced blobs. (Single-instance shorthand.)";
    };

    user = lib.mkOption {
      type = lib.types.str;
      default = "detritus";
      description = "User account used to run detritusd. (Single-instance shorthand.)";
    };

    group = lib.mkOption {
      type = lib.types.str;
      default = "detritus";
      description = "Group used to run detritusd. (Single-instance shorthand.)";
    };

    openFirewall = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Open the detritus TCP port in the host firewall. (Single-instance shorthand.)";
    };

    # -------------------------------------------------------------------------
    # Multi-instance attrset (new API)
    # -------------------------------------------------------------------------

    instances = lib.mkOption {
      type = lib.types.attrsOf (lib.types.submodule instanceSubmodule);
      default = {};
      description = ''
        Independent detritus instances. Each entry produces its own
        systemd service unit, user, group, data directory, and bound
        socket address. Use this when one host serves multiple tenants
        or environments.

        Leave empty (the default) to use the single-instance shorthand
        options above. Setting BOTH this attrset AND `enable = true`
        raises an assertion failure.
      '';
    };
  };

  config = let
    # Any instance active (either from shorthand or explicit)?
    anyActive = isShorthand || hasExplicitInstances;
  in
    lib.mkIf anyActive {
      assertions =
        [
          # Cannot mix shorthand and explicit instances.
          {
            assertion = !(cfg.enable && hasExplicitInstances);
            message = ''
              services.detritus: Set either the top-level shorthand
              (services.detritus.enable = true; …) OR
              services.detritus.instances = { … }, not both.
            '';
          }
          # Shorthand: tokensConfig must be set when using the shorthand.
          {
            assertion = !isShorthand || cfg.tokensConfig != null;
            message = ''
              services.detritus: services.detritus.tokensConfig must be set when using
              the single-instance shorthand (services.detritus.enable = true).
            '';
          }
          # Shorthand: dataDir must be in knownDataDirs.
          {
            assertion = !isShorthand || builtins.elem (toString cfg.dataDir) (map toString cfg.knownDataDirs);
            message = ''
              services.detritus.dataDir is ${toString cfg.dataDir}, which is not in
              services.detritus.knownDataDirs. This guard prevents accidental production
              data orphaning. Add the intentional path to knownDataDirs if this move is deliberate.
            '';
          }
        ]
        # Per-instance: dataDir must be in each instance's knownDataDirs.
        ++ lib.mapAttrsToList (name: inst: {
          assertion = builtins.elem (toString inst.dataDir) (map toString inst.knownDataDirs);
          message = ''
            services.detritus.instances.${name}.dataDir is ${toString inst.dataDir}, which is
            not in services.detritus.instances.${name}.knownDataDirs.
          '';
        })
        cfg.instances
        # No two instances may share the same dataDir.
        ++ (
          let
            dataDirs = map (i: toString i.dataDir) (lib.attrValues activeInstances);
            uniqueDataDirs = lib.unique dataDirs;
          in [
            {
              assertion = lib.length dataDirs == lib.length uniqueDataDirs;
              message = ''
                services.detritus: Two or more instances share the same dataDir.
                Each instance must have a unique dataDir to avoid content-addressed
                blob store corruption.
              '';
            }
          ]
        )
        # No two instances may bind the same port.
        ++ (
          let
            ports = map (i: bindPortOf i.bind) (lib.attrValues activeInstances);
            uniquePorts = lib.unique ports;
          in [
            {
              assertion = lib.length ports == lib.length uniquePorts;
              message = ''
                services.detritus: Two or more instances bind to the same TCP port.
                Each instance must have a unique bind address:port.
              '';
            }
          ]
        );

      users.users = usersAttrset;
      users.groups = groupsAttrset;

      systemd.tmpfiles.rules = tmpfilesRules;

      systemd.services = serviceAttrset;

      networking.firewall.allowedTCPPorts = activePorts;
    };
}
