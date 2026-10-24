# OA-14 disposable pg_upgrade image, built only at the final boundary.
# Build with ID-derived local aliases for the two FROM references, and pass the
# resolved local IDs separately. Runtime accepts only IDs and verifies ancestry.
# The project Dockerfiles retain their upstream/pgmq pins unchanged.
ARG PG17_IMAGE
ARG PG18_IMAGE
ARG PG17_SOURCE_ID
ARG PG18_SOURCE_ID
FROM ${PG17_IMAGE} AS source17
FROM ${PG18_IMAGE}
ARG PG17_IMAGE
ARG PG18_IMAGE
ARG PG17_SOURCE_ID
ARG PG18_SOURCE_ID
ARG PG17_PACKAGE_VERSION=17.11-1.pgdg12+2
LABEL ai.prometheus.upgrade.pg17-source="${PG17_SOURCE_ID}" \
      ai.prometheus.upgrade.pg18-source="${PG18_SOURCE_ID}" \
      ai.prometheus.upgrade.protocol="oa14-v1"

# PGDG version verified against docker-library/postgres/17/bookworm/Dockerfile
# on 2026-10-24. Installing inside this disposable image is not a host install.
RUN apt-get update \
 && apt-get install -y --no-install-recommends "postgresql-17=${PG17_PACKAGE_VERSION}" \
 && rm -rf /var/lib/apt/lists/* \
 && /usr/lib/postgresql/17/bin/postgres --version | grep -E '^postgres \(PostgreSQL\) 17\.11([[:space:]]|$)' \
 && /usr/lib/postgresql/18/bin/postgres --version | grep -E '^postgres \(PostgreSQL\) 18\.6([[:space:]]|$)'

# pgmq 1.13.1 is SQL-only (upstream Makefile DATA/PGXS); both server majors
# need their own extension control/SQL files while pg_upgrade checks catalogs.
COPY --from=source17 /usr/share/postgresql/17/extension/pgmq* /usr/share/postgresql/17/extension/
ENTRYPOINT ["/bin/bash"]
