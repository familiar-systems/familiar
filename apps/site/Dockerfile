# Multi-stage build for the Astro static site.
# Build context must be the repo root:
#   docker build -f apps/site/Dockerfile -t familiar-systems-site .

# -- Stage 1: Build ----------------------------------------------------------
FROM node:26-slim AS build

ENV PNPM_HOME="/pnpm"
ENV PATH="$PNPM_HOME:$PATH"

ARG PNPM_VERSION
RUN npm install -g pnpm@${PNPM_VERSION}

WORKDIR /app

# Copy workspace config + lockfile first (layer caching)
COPY pnpm-workspace.yaml pnpm-lock.yaml package.json ./

# pnpm-workspace.yaml declares patchedDependencies; pnpm hashes the patch
# files during install (even --frozen-lockfile), so they must be in the image.
COPY patches/ patches/

# Copy package.json for the site's workspace dependency closure. The site
# imports @familiar-systems/design's CSS; design dev-depends on tsconfig, which
# `--filter site...` pulls into the install graph, so its package.json must exist.
COPY apps/site/package.json apps/site/package.json
COPY packages/design/package.json packages/design/package.json
COPY tooling/tsconfig/package.json tooling/tsconfig/package.json

# Install deps for site and its workspace dependencies (frozen lockfile for reproducibility)
RUN pnpm install --frozen-lockfile --filter @familiar-systems/site...

# Copy source for the site and the design package it imports. tsconfig source is
# not needed: the astro build runs no tsc, and its package.json already satisfies
# the workspace symlink.
COPY apps/site/ apps/site/
COPY packages/design/ packages/design/

# Build static site + pagefind index.
# All three values are required and have no defaults; astro.config.mjs throws
# at build time if any is unset. Different values per environment (dev/preview/
# prod), so a default would silently bake the wrong apex or path prefix into
# deployed artifacts.
ARG SITE_URL
ENV SITE_URL=$SITE_URL
ARG SITE_BASE_PATH
ENV SITE_BASE_PATH=$SITE_BASE_PATH
ARG PUBLIC_APP_URL
ENV PUBLIC_APP_URL=$PUBLIC_APP_URL
RUN pnpm --filter @familiar-systems/site build

# -- Stage 2: Serve -----------------------------------------------------------
FROM nginx:1.31-alpine

COPY apps/site/nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=build /app/apps/site/dist/ /usr/share/nginx/html/

EXPOSE 80
