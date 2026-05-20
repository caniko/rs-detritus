# Cloudflare proxy ingress evaluation

This page helps you decide whether to put a detritus instance behind Cloudflare's free proxy or to expose it directly. It enumerates the relevant Cloudflare limits and quirks, states when Cloudflare is practical, and lists the alternative ingress topologies detritus's architecture supports.

## TL;DR (recommendation)

**Use Cloudflare free-tier proxying for detritus only when:**
- You enable Phase 03's client-side zstd compression (keeps real-world dumps well under 100 MB).
- You are comfortable with Cloudflare-certificate-terminated TLS at the edge.
- Your deployment does not require bidirectional streaming gRPC (detritus's `Export` RPC is unary, which Cloudflare's free tier handles).

**Use direct ingress (Tailscale, WireGuard, or a plain LE-cert reverse proxy on your own host) when:**
- You anticipate request bodies larger than 100 MB.
- You need TLS termination to stay strictly on your own infra.
- You need full HTTP/2 gRPC features beyond unary calls (server streaming, bidirectional streaming).

## What Cloudflare's free tier does and doesn't allow

### Body size limits

Cloudflare's request body size limits depend on your account plan, not on any Workers or other add-on:

| Plan       | Max Body Size |
|------------|---------------|
| Free       | 100 MB        |
| Pro        | 100 MB        |
| Business   | 200 MB        |
| Enterprise | 500 MB        |

**Implication for detritus:** A panic tarball or minidump that exceeds 100 MB on the wire (after compression) will be rejected by Cloudflare with HTTP 413 before it ever reaches your detritus instance. Track upload-rejection metrics on the client side to catch this early.

### gRPC support

- **HTTP/2 transit:** Fully supported.
- **Unary gRPC:** Works on all tiers, including Free.
- **Server-streaming and bidirectional streaming:** Not officially documented as restricted on free tier, but historically had quirks; verify against your detritus deployment if you plan to use streaming beyond unary calls.
- **Enabling gRPC:** Log into the Cloudflare dashboard, navigate to **Network settings**, and toggle **gRPC** to **On**.
- **gRPC-Web:** Not used by detritus (which uses native gRPC over HTTP/2, not the JS-browser bridge). Understanding the distinction helps when reading Cloudflare's docs.

### WebSocket

Not used by detritus; not relevant.

### TLS and origin certificates

- **Cloudflare's edge:** Terminates TLS with a Cloudflare-issued universal SSL certificate. Your users' connections are encrypted to Cloudflare.
- **Origin connection:** Use either "Full" or "Full (strict)" SSL/TLS mode in your Cloudflare zone settings. "Full (strict)" requires a valid origin certificate; use a Let's Encrypt cert (recommended for public detritus instances) or a Cloudflare Origin Certificate.
- **Data visibility:** Logs and crash payloads are decrypted at Cloudflare's edge. If that's unacceptable for your data (e.g., for compliance reasons), do not use Cloudflare; use direct ingress instead.

### Request timeouts

- **Free tier:** 100 seconds max per request.
- **Implication:** Long-running uploads of large dumps over slow links may time out. Client-side zstd compression (Phase 03) mitigates this by reducing payload size.

### Rate limiting

Cloudflare free tier includes basic rate-limiting controls. Configure a coarse anti-abuse limit at the Cloudflare edge, but rely on detritus's own per-token rate limiter as the authority for per-token quotas.

## How this interacts with detritus's architecture

| Architecture feature | Cloudflare implication |
|---|---|
| **One process, one port** ([architecture.md](../architecture.md)) | Cloudflare's single-hostname routing maps cleanly. |
| **Mandatory wire-level gzip** ([architecture.md](../architecture.md)) | Cloudflare passes gzip through; may negotiate brotli for browser clients (irrelevant for gRPC). |
| **Per-token bearer auth** ([architecture.md](../architecture.md)) | Cloudflare does not inspect or transform `Authorization` headers; pass through as-is. |
| **Phase 03 client-side zstd** | Shrinks dumps far below the 100 MB cap. |
| **Phase 04 multi-instance NixOS module** | Each instance needs its own hostname OR a path-based router. Cloudflare Workers can implement the latter on free tier with one Worker route per instance. |

## Alternative ingress topologies

### 1. Direct on public IPv4/IPv6

Plain reverse proxy (caddy, nginx) on your host with a Let's Encrypt certificate; detritusd listens on `127.0.0.1`.

**Pros:** Full control, no body-size cap, TLS on your hardware.
**Cons:** Public endpoint visible to the internet; requires firewall and DDoS mitigation.

### 2. Tailscale or WireGuard tailnet

detritusd listens on a tailnet IP; only authorized clients have routes.

**Pros:** No public exposure, no Cloudflare body cap, only clients in your tailnet can reach it.
**Cons:** Every client must also run Tailscale / WireGuard. Best for solo-operated services or teams where all peers are already on the tailnet.

### 3. Cloudflare Tunnel (cloudflared)

Outbound-only tunnel from your host to Cloudflare; no inbound port forwarding needed.

**Pros:** Bypasses NAT/firewall constraints; works behind strict corporate networks.
**Cons:** Same 100 MB free-tier body cap applies; Cloudflare still decrypts TLS at the edge.

### 4. SSH reverse port-forward

Forward a remote SSH session's local port back to your detritusd.

**Pros:** Ad-hoc, useful for testing.
**Cons:** Not suitable for production; manual setup on each connection.

## Practical guidance (as of 2026)

**For solo/small-project deployments where dumps are consistently < 30 MB after zstd:**
- Cloudflare free-tier proxy is practical.
- Use "Full (strict)" TLS with a Let's Encrypt cert on your origin.
- Enable Cloudflare zone setting "Network → gRPC: On".
- Document the Cloudflare account and zone in your ops runbook.

**For deployments where dumps may exceed 100 MB, TLS termination must stay on-origin, or operators already run Tailscale:**
- Use Tailscale, direct ingress, or Cloudflare Tunnel (understanding that Tunnel has the same 100 MB cap).
- Cloudflare free proxy is not pulling its weight at that scale.

## Re-evaluation triggers

Revisit this decision if:
- A real-world dump is rejected with HTTP 413 (payload size exceeded).
- Cloudflare changes the free-tier body cap (monitor release notes at [developers.cloudflare.com](https://developers.cloudflare.com/)).
- The crash multipart format evolves to add parts that push typical uploads near the cap.
- Your data governance or compliance requirements mandate origin-side TLS termination.

## References

- [Cloudflare request body size limits](https://developers.cloudflare.com/workers/platform/limits/)
- [Cloudflare gRPC connections](https://developers.cloudflare.com/network/grpc-connections/)
- [detritus architecture](../architecture.md)
- [Phase 03: Client-side zstd compression](../planning/multi-tenant-validation-and-compression/03-client-side-zstd-compression.md)
