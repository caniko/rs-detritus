# NixOS Module

The flake exports a NixOS module for running `detritusd` in either a single-instance shorthand mode or an explicit multi-instance mode.

## Flake Outputs

The repository exports:

```text
packages.<system>.detritus
packages.<system>.docs
packages.<system>.site
nixosModules.default
nixosConfigurations.detritus-test-vm
nixosConfigurations.detritus-multi-test-vm
```

## Single-Instance Shorthand

The existing shorthand interface is:

```nix
services.detritus = {
  enable = true;
  bind = "127.0.0.1:4317";
  tokensConfig = config.age.secrets.detritus-tokens.path;
  logsTtlDays = 14;
  crashesTtlDays = 90;
};
```

This produces a single `detritus.service` unit and keeps the historic `detritus:detritus` runtime identity.

## Multi-Instance Mode

Explicit instances use `services.detritus.instances`:

```nix
services.detritus.instances = {
  acme = {
    bind = "0.0.0.0:4317";
    dataDir = "/var/lib/detritus-acme";
    tokensConfig = "/run/secrets/detritus-acme.toml";
    user = "detritus-acme";
    group = "detritus-acme";
    openFirewall = true;
  };

  beta = {
    bind = "0.0.0.0:4318";
    dataDir = "/var/lib/detritus-beta";
    tokensConfig = "/run/secrets/detritus-beta.toml";
  };
};
```

Each explicit instance produces its own:

- `detritus-<name>.service`
- system user and group
- token preflight script
- firewall opening when `openFirewall = true`

## Important Guards

The module enforces several safety checks at evaluation time:

- you cannot mix the top-level shorthand with `services.detritus.instances`
- `dataDir` must be listed in `knownDataDirs`
- explicit instances may not share the same `dataDir`
- explicit instances may not bind the same TCP port

These guards exist to prevent silent data orphaning and port collisions during upgrades.

## Schema Files

Each instance optionally accepts `schemaDir`.

When set, the module:

- grants the service read-only access to that path
- expects schema file references in `tokensConfig` to resolve inside that directory tree

## Runtime Contract

The systemd unit refuses to start if `tokensConfig` is not mode `0400` and owned by the configured service user and group.

By default the service stores persistent data in `/var/lib/detritus` for the shorthand mode and `/var/lib/detritus-<name>` for explicit instances.
