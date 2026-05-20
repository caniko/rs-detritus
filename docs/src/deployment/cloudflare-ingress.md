# Cloudflare Ingress

This page explains when Cloudflare's hosted edge is a practical ingress layer for Detritus and when you should keep TLS termination on your own infrastructure.

For the self-hosted alternative, see [Caddy Reverse Proxy](./caddy-reverse-proxy.md).

## Recommendation

Cloudflare's free proxy is practical when all of these are true:

- crash uploads stay comfortably below the `100 MB` free-tier body limit after zstd compression
- unary OTLP/gRPC is sufficient
- TLS termination at Cloudflare's edge is acceptable for your data

Prefer direct ingress, Tailscale, WireGuard, or another self-hosted reverse proxy when any of these are false.

## Relevant Cloudflare Limits

### Body Size

Free and Pro plans cap request bodies at `100 MB`.

For Detritus, that means an oversized crash upload is rejected with HTTP `413` before it reaches `detritusd`.

### gRPC

Detritus uses unary OTLP/gRPC log export, which Cloudflare's HTTP/2 transit supports.

If your deployment later needs bidirectional or server-streaming gRPC behavior, re-evaluate the proxy decision against Cloudflare's current product behavior.

### TLS

Cloudflare terminates TLS at the edge. The origin leg should run in `Full` or `Full (strict)` mode with a valid origin certificate.

If decrypting crash or log payloads at Cloudflare is unacceptable, do not use Cloudflare for Detritus ingress.

### Request Timeouts

Long uploads over slow links can time out at the Cloudflare edge. Payload-level zstd compression helps by shrinking crash bodies before upload.

## Alternative Topologies

- direct public ingress with your own reverse proxy and certificate
- a Tailscale or WireGuard tailnet
- Cloudflare Tunnel, if the network path matters more than the edge data-visibility tradeoff

## Re-Evaluate When

Revisit the decision if:

- real-world uploads hit HTTP `413`
- typical crash sizes grow toward the plan limit
- your compliance requirements change
- your ingest surface expands beyond unary gRPC and HTTP multipart uploads
