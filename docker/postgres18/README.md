# parking-lot-pg images (CR-43)

PostgreSQL + pgmq for tests and local development. `docker/postgres17` is the matrix variant;
both share `postgres18/postgresql.conf` and `postgres18/init/` (build context is `docker/`).

| Tag | Base (pinned index digest) | pgmq |
|---|---|---|
| `parking-lot-pg:18` | `postgres:18-bookworm@sha256:bbf83594798e144bff365a9c191c5d779f181fb3135902064ab8ff11918020fc` (18.6, PGDG-built) | v1.13.1 (`8eab182`) |
| `parking-lot-pg:17` | `postgres:17-bookworm@sha256:858bb5e98eed02eb8cafbcdc007f9a65d1b613972028944a8ec2133ccaf80d27` (17.11, PGDG-built) | v1.13.1 (`8eab182`) |

The official image installs `postgresql-N` from apt.postgresql.org (version string `1.pgdg12+2`).
pgmq is SQL-only (PGXS `make install`), supports PG 14-18, and is checked against its commit SHA at build.

Build: `docker build -f docker/postgres18/Dockerfile -t parking-lot-pg:18 docker/` (likewise 17).
Bump the digest/pgmq tag deliberately; each setting in `postgresql.conf` is commented with its reason.

CI (`postgres` job, cr-36) builds both images with buildx and runs every Docker-gated Postgres test on each; set `PARKING_LOT_PG_MAJOR=17|18` to pick the image locally. Operations: `docs/runbooks/postgres18.md`.
