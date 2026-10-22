# Documentation site container: static build served by a non-root nginx.
# Build:  docker build -t prometheus-parking-lot-docs website
# Run:    docker run --rm -p 8080:8080 prometheus-parking-lot-docs
# Pages-style base path: --build-arg BASE_URL=/prometheus-parking-lot-rs/

FROM node:24.11.1-alpine AS build
WORKDIR /site
COPY package.json package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY . .
ARG SITE_URL=http://localhost:8080
ARG BASE_URL=/
ENV SITE_URL=${SITE_URL} BASE_URL=${BASE_URL}
RUN npm run build

FROM nginxinc/nginx-unprivileged:1.29-alpine AS runtime
ARG BASE_URL=/
# nginx-unprivileged runs as uid 101 and listens on 8080.
COPY nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=build --chown=101:101 /site/build /usr/share/nginx/html${BASE_URL}
USER 101
EXPOSE 8080
