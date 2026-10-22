# parking-lot-pg: PostgreSQL 18 + pgmq for tests and local development.
#
# Base image: the official `postgres:18-bookworm` image. Its server packages come
# from the PGDG apt repository (apt.postgresql.org), i.e. it is PGDG-built; the
# multi-arch index is pinned by digest below.
# No Flint extensions (pgrx, pg_net, vault, ...) are included.
#
# Build (context = docker/):
#   docker build -f docker/postgres18/Dockerfile -t parking-lot-pg:18 docker/
#
# Structure modelled on flint-forge images/postgres18: multi-stage, extension
# built from a pinned source tag in a throwaway stage, copied into the runtime.

ARG PG_BASE=postgres:18-bookworm@sha256:bbf83594798e144bff365a9c191c5d779f181fb3135902064ab8ff11918020fc

# ---- Stage: pgmq (SQL-only extension, built with PGXS) ----------------------
FROM ${PG_BASE} AS pgmq
ARG PG_MAJOR_VERSION=18
# pgmq 1.13.1 supports PG 14-18 (README / CI matrix). The tag is checked
# against the commit below so a moved tag fails the build.
ARG PGMQ_VERSION=v1.13.1
ARG PGMQ_COMMIT=8eab18272e50077ab9f72d9787296655f2849dcc
RUN apt-get update \
 && apt-get install -y --no-install-recommends git make gcc postgresql-server-dev-${PG_MAJOR_VERSION} ca-certificates \
 && rm -rf /var/lib/apt/lists/*
RUN git clone --depth 1 --branch "${PGMQ_VERSION}" https://github.com/pgmq/pgmq /src/pgmq \
 && test "$(git -C /src/pgmq rev-parse HEAD)" = "${PGMQ_COMMIT}"
WORKDIR /src/pgmq/pgmq-extension
RUN make PG_CONFIG=/usr/lib/postgresql/${PG_MAJOR_VERSION}/bin/pg_config \
 && make PG_CONFIG=/usr/lib/postgresql/${PG_MAJOR_VERSION}/bin/pg_config install DESTDIR=/out

# ---- Stage: runtime ----------------------------------------------------------
FROM ${PG_BASE}
ARG PGMQ_VERSION=v1.13.1
LABEL org.opencontainers.image.title="parking-lot-pg:18" \
      org.opencontainers.image.description="PostgreSQL 18 with pgmq for prometheus-parking-lot tests" \
      ai.prometheus.pgmq.version="${PGMQ_VERSION}" \
      ai.prometheus.pg.major="18"

COPY --from=pgmq /out/ /

# Same tuned config and init scripts for every PG major (docker/postgres18 owns them).
COPY postgres18/postgresql.conf /etc/postgresql/postgresql.conf
COPY postgres18/init/ /docker-entrypoint-initdb.d/

CMD ["postgres", "-c", "config_file=/etc/postgresql/postgresql.conf"]
