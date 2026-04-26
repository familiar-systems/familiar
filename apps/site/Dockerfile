# Multi-stage build for the Astro static site.
# Build context must be the repo root:
#   docker build -f apps/site/Dockerfile -t familiar-systems-site .

# -- Stage 1: Build ----------------------------------------------------------
FROM node:24-slim AS build

ENV PNPM_HOME="/pnpm"
ENV PATH="$PNPM_HOME:$PATH"

ARG PNPM_VERSION
RUN npm install -g pnpm@${PNPM_VERSION}

WORKDIR /app

# Copy workspace config + lockfile first (layer caching)
COPY pnpm-workspace.yaml pnpm-lock.yaml package.json ./

# Copy only the site's package.json (no other apps/packages needed at install)
COPY apps/site/package.json apps/site/package.json

# Install deps for site only (frozen lockfile for reproducibility)
RUN pnpm install --frozen-lockfile --filter @familiar-systems/site...

# Copy site source
COPY apps/site/ apps/site/

# Build static site + pagefind index
ARG SITE_URL=https://preview.familiar.systems
ENV SITE_URL=$SITE_URL
# Required: site.config.ts throws at build time if PUBLIC_APP_URL is unset.
# No default — the value differs across dev/preview/prod and a fallback
# would silently bake the wrong apex into deployed artifacts.
ARG PUBLIC_APP_URL
ENV PUBLIC_APP_URL=$PUBLIC_APP_URL
RUN pnpm --filter @familiar-systems/site build

# -- Stage 2: Serve -----------------------------------------------------------
FROM nginx:alpine

COPY --from=build /app/apps/site/dist/ /usr/share/nginx/html/

EXPOSE 80
