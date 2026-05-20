# Caddy Reverse Proxy

This page documents the self-hosted ingress path for Detritus on canix hosts.

## Topology

Caddy terminates public TLS and forwards cleartext loopback traffic to `detritusd`.

```text
client
  |
  | HTTPS
  v
Caddy :443
  |
  | plain HTTP / h2c on loopback
  v
detritusd 127.0.0.1:4317
```

The same upstream listener carries both OTLP/gRPC logs and multipart crash uploads.

## Receiver Bind

The correct bind address behind Caddy is:

```sh
detritusd --bind 127.0.0.1:4317 --tokens-config /etc/detritus/tokens.toml
```

Do not bind `0.0.0.0:4317` just to make the proxy work.

## canix Route Shape

The canix Caddy preset uses `canix.presets.services.caddy.mkReverseProxyRoute`. A Detritus host configuration looks like this:

```nix
{
  config,
  inputs,
  lib,
  ...
}: let
  hostname = "detritus.example.com";
  bindAddr = "127.0.0.1";
  bindPort = 4317;
in {
  imports = [
    inputs.detritus.nixosModules.default
  ];

  services.detritus = {
    enable = true;
    bind = "${bindAddr}:${toString bindPort}";
    tokensConfig = config.age.secrets.detritus-tokens.path;
  };

  canix.presets.services.caddy.routes = [
    (lib.recursiveUpdate
      (config.canix.presets.services.caddy.mkReverseProxyRoute {
        inherit hostname;
        port = bindPort;
        host = bindAddr;
      })
      {
        handle = [
          {
            handler = "reverse_proxy";
            transport = {
              protocol = "http";
              versions = ["h2c" "2"];
            };
            upstreams = [
              { dial = "${bindAddr}:${toString bindPort}"; }
            ];
          }
        ];
      })
  ];
}
```

The important part is the `h2c` upstream transport, which lets `tonic` keep serving gRPC over cleartext loopback.

## Body Size And Timeouts

The server accepts up to `100 * 1024 * 1024` bytes for each dump or attachment part and applies a request-wide `150 MiB` body limit.

If you add a Caddy-side request-body limit, keep it aligned with the server request limit:

```text
request_body {
  max_size 150MiB
}
```

## Client Configuration

Clients should point at the public HTTPS origin, not the loopback listener.

`detritus-client` calls `detritus::install_default_crypto_provider()` before it builds its HTTPS crash shipper. Applications that construct their own `reqwest` clients earlier in process startup need to call that function themselves before the first TLS handshake.

## Health Check

The receiver exposes `GET /healthz`, not `/health`.

Through Caddy:

```sh
curl -i https://detritus.example.com/healthz
```
