# Phase 01 — Exhaustive publish-readiness audit + decisions RFC

> **Recommended Codex model: GPT 5.5 high**
>
> This phase is the project-shaping work. It must inventory every
> publish-blocking gap across three crates, decide which surface
> is `pub` vs `pub(crate)`, pick semver commitments, choose
> categories from the crates.io taxonomy, decide whether
> `detritus-server` ships as a binary crate or stays internal,
> and produce a decisions document that the next six phases
> execute against without re-litigating. Wrong calls (e.g. an
> over-broad `pub` surface, a category that doesn't fit) cost a
> follow-up major version. `high` effort buys the design
> headroom; `max` is overkill because the surface is bounded to
> three crates.

## Working tree

`/data/nvme0/can/Projects/detritus`. The detritus repo. This is
the only phase that lands purely a documentation commit; every
subsequent phase consumes the decisions doc.

## Goal

A single committed RFC document
(`docs/planning/detritus-crates-io-finalization/_publish-readiness-rfc.md`)
that, for each of the three workspace crates, enumerates:

- Whether it is published or kept internal.
- Its claimed crates.io name (current name verified available on
  crates.io OR an alternative listed as fallback).
- Its proposed `description`, `keywords` (≤ 5), `categories`
  (from crates.io's fixed taxonomy), `homepage`,
  `documentation`, `readme`.
- The locked-down public API surface (a list of items
  intentionally `pub`, with everything else demoted to
  `pub(crate)` or removed in Phase 04).
- The semver commitment (what counts as a breaking change for
  this v0.1.0 / v0.x trajectory; when v1.0.0 ships).
- Workspace-level decisions: MSRV value, `[workspace.lints]`
  policy, feature-flag philosophy, and (informational only,
  since Apache-2.0 is permissive) any dep with an
  `UNKNOWN`/proprietary license that should be addressed
  before publish.

## Why this matters now

Today the detritus workspace has *zero* crates.io-ready metadata
on any of its three crates:

```
$ grep -n "description\|keywords\|categories\|homepage\|documentation\|readme" \
    Cargo.toml crates/*/Cargo.toml
# (zero matches)
```

License currently declares `MIT OR Apache-2.0` but ships no
LICENSE text — the user has chosen to consolidate to
`Apache-2.0` and ship the canonical text (Phase 02). Per-crate
path deps lack `version = "..."` qualifiers, so `cargo publish`
will reject them. There is no CI, no CHANGELOG, no per-crate
README, no examples directory.
`detritus-client` lifts `#![warn(missing_docs)]` but the sibling
crates do not. Public re-exports leak internal types that may
not survive the first downstream user.

### Cost of deferring

- Every subsequent phase (license switch, metadata fill, README
  authoring, CI, dry-run) makes ad-hoc decisions on what to
  expose and how to label it. Those decisions compound and
  trigger rework when they conflict.
- A premature v0.1.0 publish locks in the public API surface; a
  later cleanup forces a v0.2.0 *and* a deprecation cycle
  before crates.io users notice.
- Without a decisions doc, a future agent has no audit trail for
  why (say) `detritus-server` is or isn't published.

## Out of scope

- Implementing any of the changes the RFC recommends — those are
  Phases 02-07.
- Touching source code beyond reading it for the audit.
- Renaming crates or restructuring the workspace topology. If
  the RFC concludes a rename is needed, that's a follow-up plan,
  not part of this set.
- Changing dependencies (versions, additions, removals). The
  RFC may flag dependency-license incompatibilities under
  publishability, but resolution lands in a later phase.

## Plan

1. **Workspace inventory.** Tabulate every workspace crate with:
   - `name`, `lib.name` if overridden, `package.name`, version,
     edition, current license.
   - Public re-exports from `src/lib.rs` (`grep -nE "^pub use" src/lib.rs`).
   - Public modules (`grep -nE "^pub mod" src/lib.rs`).
   - Module count and total LOC under `src/`.
   - Test files under `tests/`.
   - Whether the crate produces a `[[bin]]`.
   - Whether `#![warn(missing_docs)]` is set.
   - Dependency count and any path deps (`grep -nE 'path = "' Cargo.toml`).

2. **Per-crate publish decision.** For each crate, set verdict
   ∈ {PUBLISH-v0.1.0, PUBLISH-LATER, INTERNAL-ONLY}. Rationale
   per crate, including:
   - Who would `cargo add` this crate (the persona)?
   - Is the API stable enough for v0.x semver?
   - Are there transitive dep-license blockers?

   Default: all three publish at v0.1.0. Suspend any with
   identified blockers and call them out explicitly.

3. **crates.io name reservation.** For each PUBLISH crate, the
   name must be free on crates.io. The audit cannot fetch
   crates.io from this offline session — record the proposed
   name and an explicit "verify-before-publish" line in the
   RFC. Phase 07 (dry-run) will reserve the name. If a name is
   already taken, the RFC lists fallback names in priority
   order:

   - `detritus-protocol` → fallbacks: `detritus-wire`, `detritus-types`
   - `detritus-client` (lib name `detritus`) → fallbacks: `detritus-sdk`, `detritus-tracing`
   - `detritus-server` (bin `detritusd`) → fallbacks: `detritusd`, `detritus-receiver`

4. **Public-API audit.** For each PUBLISH crate, list every
   `pub` item by category:
   - **Stable surface** — explicitly committed to semver
     (named in lib.rs `//!` docstring, used by the documented
     example, or part of the builder API).
   - **Re-exports** — items re-exported from internal modules;
     decide whether to keep public or demote.
   - **Inadvertent `pub`** — items technically public because
     the module is `pub mod` but never named in docstring or
     README; default to demote in Phase 04.

   Example for `detritus-protocol`:
   - Currently `pub mod crash`, `pub mod multipart`, `pub mod
     otlp`, `pub mod source` — the module structure leaks
     auto-generated OTLP proto types and multipart streaming
     internals. Decision: keep `crash::*` and `source::*`
     re-exports public; demote `otlp` and `multipart` to
     `pub(crate)` if no downstream user needs them, OR document
     them as advanced surface with a stability disclaimer.

5. **Categories + keywords.** crates.io accepts up to 5
   keywords (lowercase, ≤ 20 chars each) and an unbounded but
   small number of categories from a fixed taxonomy. Pick:

   - `detritus-protocol`: candidates: `network-programming`,
     `encoding`, `parser-implementations`. Keywords:
     `observability`, `otlp`, `telemetry`, `crash-reporting`,
     `protocol`.
   - `detritus-client`: candidates: `development-tools::debugging`,
     `development-tools::profiling`, `web-programming`.
     Keywords: `observability`, `tracing-subscriber`,
     `crash-reporting`, `panic-hook`, `telemetry`.
   - `detritus-server`: candidates:
     `command-line-utilities`, `web-programming::http-server`.
     Keywords: `observability`, `otlp`, `crash-reporting`,
     `server`, `telemetry`.

   The RFC records the final choices.

6. **MSRV decision.** The workspace uses `edition = "2024"` and
   `resolver = "3"` — both require recent toolchains. Set
   `rust-version` to the minimum that builds clean on stable.
   Default: `rust-version = "1.85"` (the first stable that
   supports edition 2024). Validate in the RFC by referencing
   the edition-2024 stabilization release notes.

7. **Workspace lints policy.** The
   `[workspace.lints.rust]` block is empty. Decide on a
   policy. Default for a publish-bound observability SDK:

   ```toml
   [workspace.lints.rust]
   missing_docs = "warn"
   unsafe_code = "forbid"
   rust_2018_idioms = "warn"
   nonstandard_style = "warn"
   future_incompatible = "warn"

   [workspace.lints.clippy]
   all = { level = "warn", priority = -1 }
   pedantic = { level = "warn", priority = -1 }
   nursery = { level = "warn", priority = -1 }
   # ... project-specific allow-list of pedantic lints we don't enforce
   ```

   The RFC names the exact lint table; Phase 03 lifts it
   verbatim into `Cargo.toml`.

8. **Dep-license sanity check.** Apache-2.0 is permissive, so
   consuming any combination of permissive or copyleft deps
   is fine — the published `.crate` archives contain only our
   own source. Walk `cargo tree -e all --workspace --depth 1`
   (or read each `Cargo.toml`'s dependency list) and record
   each direct dep's SPDX license, primarily to surface any
   `UNKNOWN` / proprietary / exotic entries that warrant
   investigation before publish. For a typical Rust
   observability stack — `prost`, `tonic`, `axum`, `tokio`,
   `tracing`, `chrono`, `serde`, `uuid` — every dep should be
   `MIT`, `Apache-2.0`, or `MIT OR Apache-2.0`. Flag any
   outlier for Phase 02's commit body.

9. **`detritus-server` ship-as-binary-crate decision.** Two
   paths:
   - **Publish** the server crate so users can
     `cargo install detritus-server`. Requires keeping the
     server's deps publish-clean.
   - **Internal**: keep server out of crates.io; users
     consume from source. Server stays in the workspace; only
     `detritus-protocol` and `detritus-client` ship.

   Default: PUBLISH. Operator distribution via `cargo install`
   is a real-world ask. The RFC names the decision.

10. **CI policy + release cadence.** Sketch (one paragraph
    each):
    - CI on Codeberg/Forgejo per project conventions.
    - Release cadence (manual `cargo publish` on tag, or
      automated). Default: manual, tag-driven.
    - Version-bump policy (synchronize all three at the same
      version, or independent). Default: synchronize during
      v0.x.

11. **Write the RFC.** File path:
    `docs/planning/detritus-crates-io-finalization/_publish-readiness-rfc.md`
    (underscore prefix marks it as a non-phase artifact).

    Structure:

    ```markdown
    # RFC: Detritus crates.io publish readiness

    Date: <YYYY-MM-DD>
    Status: ADOPTED — feeds Phases 02–07

    ## Workspace inventory
       (step 1 table)
    ## Per-crate publish decisions
       (step 2 verdicts)
    ## Crate names + fallbacks
       (step 3)
    ## Public-API surface per crate
       (step 4)
    ## crates.io metadata (categories, keywords, descriptions)
       (step 5)
    ## MSRV + edition decisions
       (step 6)
    ## Workspace lints policy
       (step 7 — paste the exact table)
    ## Dependency-license sanity check
       (step 8 — Apache-2.0 is permissive; informational only)
    ## detritus-server distribution decision
       (step 9)
    ## CI policy + release cadence
       (step 10)
    ## Open questions
       (anything not resolved — must be empty before merging)
    ```

12. **Commit.** Single docs commit on a fresh branch (avoid
    sweeping the dirty WIP shown in `git status`). Subject:

    ```
    docs/planning: detritus crates.io publish-readiness RFC
    ```

## Acceptance criteria

- [ ] `docs/planning/detritus-crates-io-finalization/_publish-readiness-rfc.md` exists.
- [ ] The RFC's workspace-inventory table lists all three current crates with name, lib name, version, edition, license, LOC, public re-export list, and dep count.
- [ ] Every crate has an explicit verdict ∈ {PUBLISH-v0.1.0, PUBLISH-LATER, INTERNAL-ONLY} with a 1-paragraph rationale.
- [ ] For each PUBLISH crate, the RFC names: description (≤ 80 chars), keywords (≤ 5, lowercased, ≤ 20 chars each), categories (from crates.io's taxonomy), homepage URL, documentation URL.
- [ ] For each PUBLISH crate, the RFC enumerates the public-API items in three buckets (Stable / Re-export / Inadvertent) so Phase 04 can demote inadvertent items.
- [ ] MSRV is named with a specific version (e.g. `1.85`) and a one-sentence justification.
- [ ] The workspace lints table is fully written (no `...` placeholders); Phase 03 lifts it as-is.
- [ ] The dep-license sanity-check table includes every direct dependency in every crate's Cargo.toml, with its SPDX license. Any `UNKNOWN` / proprietary / exotic entry is flagged for Phase 02's attention. (Apache-2.0 is permissive — no compatibility verdict required, just license-string visibility.)
- [ ] `detritus-server`'s ship-as-binary-crate decision is recorded with explicit rationale.
- [ ] The "Open questions" section is **empty** at merge time. Every open call has a resolution in the RFC body.
- [ ] No code changes in this commit. The diff is a single new file under `docs/planning/`.

## Files likely touched

In `/data/nvme0/can/Projects/detritus`:

- New: `docs/planning/detritus-crates-io-finalization/_publish-readiness-rfc.md`.

Read-only references (audited, not edited):

- `Cargo.toml` and `crates/*/Cargo.toml`.
- `crates/*/src/lib.rs` (public surface).
- `crates/*/tests/` (test inventory).
- `flake.nix` (current Nix `meta.license`).
- `docs/architecture.md`, `docs/operations.md`, `docs/storage.md`.
- `README.md` (current 14-line stub).

## Pitfalls

- **Auditing the dirty checkout.** `git status` shows modified
  files across every crate. The audit's "current state" must be
  taken from `git show HEAD:<path>` (committed state), not the
  working tree, OR explicitly note "WIP local changes excluded
  from audit". Recording untracked WIP as "current state" is a
  bug.

- **Choosing categories that don't exist.** crates.io's category
  taxonomy is a fixed list (browseable at
  `https://crates.io/categories`). Inventing a category like
  `observability-sdks` causes `cargo publish` to silently drop
  it. Stick to canonical slugs:
  `network-programming`, `web-programming::http-server`,
  `command-line-utilities`,
  `development-tools::debugging`, etc.

- **Keyword limit violation.** crates.io rejects keywords longer
  than 20 chars or with non-`[a-z0-9-]` chars. Test each
  candidate against the regex before committing the list.

- **Over-eager MSRV.** Setting `rust-version = "1.85"` works
  today but locks out users on older stable. Match it to the
  minimum the workspace actually requires; if the workspace
  doesn't use any post-1.85 feature, an older MSRV is friendlier.
  Verify by attempting a build with `rustup run 1.85.0 cargo
  build --workspace` before committing the MSRV value.

- **Inflating the public surface.** The temptation in an audit
  is to leave everything `pub` for "flexibility." For a v0.1.0,
  the smallest viable surface is the right answer — every
  unnecessary `pub` item becomes a semver commitment. Default
  bias: demote.

- **Skipping the "fallback name" planning.** If
  `detritus-client` is taken on crates.io (likely — `detritus`
  alone is also commonly squatted), Phase 07 will surface this
  at the worst time. Plan fallbacks now; the RFC must list
  them.

## Reference

- crates.io taxonomy: <https://crates.io/categories> (consult before merging the RFC).
- Apache-2.0 text (the user's chosen license): <https://www.apache.org/licenses/LICENSE-2.0.txt>.
- Detritus repo state: codeberg.org/caniko/rs-detritus (branch `trunk`).
- Project conventions: regicide consumes detritus as a git/path dep currently; once published, regicide pins to the crates.io version.
- Sequencing: this phase is the prerequisite for Phases 02–07. Their content choices reference this RFC by section.
