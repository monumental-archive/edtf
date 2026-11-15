# edtf-postgres

Postgres extension (pgrx) exposing
[`edtf-core`](https://crates.io/crates/edtf-core) in SQL — the same
validator the application runs via WebAssembly, so the two layers can never
diverge.

SQL surface: `edtf_valid(text)`, `edtf_level(text)`,
`edtf_canonical(text)`, `edtf_min(text)` / `edtf_max(text)` (index-friendly
date bounds), and `edtf_relation(text, text)` (three-valued temporal
relations).

## Installing

Prebuilt, attested tarballs are attached to each release, for Postgres
14–18 on `amd64` and `arm64` — no Rust toolchain and no `cargo-pgrx`
required. Releases are `v*` tags from v1.3.4 on; v1.1.0 to v1.2.3 are the
older `edtf-postgres-v*` tags. Download the tarball for your major and
architecture, verify it, extract its `pkgroot/` tree into `/`, then:

```sql
CREATE EXTENSION edtf_postgres;
```

The extension is `trusted`, so any user with `CREATE` on the database can
install it. v1.3.4 ships the library with its debug info inside; releases
after it ship it stripped, with a `…-debug.tar.gz` sibling beside each
tarball. The exact
commands, the support matrix and the glibc floor are in
the [repository README](https://github.com/monumental-archive/edtf#installing-the-postgres-extension).

OCI images are also published to
`ghcr.io/monumental-archive/edtf-postgres`, built from the same release
and attested the same way. One image per Postgres major, multi-arch,
tagged extension version × major: `1.3.4-pg18-artifact`. There is no
floating tag and no runnable image. Pin the digest.

The image is `FROM scratch`: the extension files and nothing else. It
carries no OS packages, so it has no inherited CVE surface, and a cold
`COPY --from` pulls only the extension rather than a whole base image.

**To deploy, build your own image from it** onto a base you pin and
refresh on your own cadence. That keeps the extension's currency and your
base's currency independent, which is the point: an extension release
should not be what ships you a base image update, and a base image update
should not wait on an extension release.

```dockerfile
FROM ghcr.io/monumental-archive/edtf-postgres:1.3.4-pg18-artifact AS ext
FROM postgres:18-trixie
COPY --from=ext /lib/ /usr/lib/postgresql/18/lib/
COPY --from=ext /share/extension/ /usr/share/postgresql/18/extension/
```

The image holds the files once, in one layout on every major: `lib/` and
`share/extension/` at the image root. The `COPY --from` above names your
server's own directories as the destination; the same layout is the
[CloudNativePG extension-ImageVolume][cnpg-ext] one, so the same tag can
be mounted into a CNPG cluster without a repack:

```yaml
postgresql:
  extensions:
    - name: edtf
      image:
        reference: ghcr.io/monumental-archive/edtf-postgres:1.3.4-pg18-artifact
```

Every release builds that layout into each image. Up to v1.2.3 a
weekly job also mounted the image into a real CNPG cluster and installed
the extension as a non-superuser; that end-to-end run is not part of the
current release path. It needs PostgreSQL 18 — `extension_control_path`
is what makes out-of-tree extensions loadable at all — CloudNativePG 1.29
or later, and a Kubernetes with image volumes: 1.35 and later have them
on by default, 1.33 and 1.34 need the `ImageVolume` feature gate. The
operand image must be Debian 12 or newer, the same glibc 2.36 floor the
tarballs carry.

Images published before the move are at
`ghcr.io/carlallenn/edtf-postgres` and stop at 1.2.3.

The tarballs remain the primary artifact; the images are wrappers around
them, never a replacement.

[cnpg-ext]: https://cloudnative-pg.io/docs/1.29/imagevolume_extensions/

Building from source instead needs `cargo-pgrx` and an initialised
`$PGRX_HOME`; see the repository for the development workflow.

Its contract is the SQL surface, not a Rust API. Part of the
[`edtf`](https://github.com/monumental-archive/edtf) crate family.
