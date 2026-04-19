# syntax=docker/dockerfile:1.7
# Multi-stage build for the Vite SPA.
# Build context must be the repo root:
#   docker build -f apps/web/Dockerfile -t familiar-systems-web .

# -- Stage 1: Build ----------------------------------------------------------
FROM node:24-bookworm-slim AS builder

RUN corepack enable

WORKDIR /build

# Copy workspace config + lockfile first (layer caching)
COPY pnpm-workspace.yaml pnpm-lock.yaml package.json ./

# Copy only the package.json files for the web build dependency tree
COPY apps/web/package.json apps/web/package.json
COPY packages/editor/package.json packages/editor/package.json
COPY packages/types-app/package.json packages/types-app/package.json
COPY packages/types-campaign/package.json packages/types-campaign/package.json
COPY tooling/tsconfig/package.json tooling/tsconfig/package.json

# Install deps for web and its workspace dependencies (frozen lockfile for reproducibility)
RUN --mount=type=cache,target=/root/.local/share/pnpm/store \
    pnpm install --frozen-lockfile --filter @familiar-systems/web...

# Copy source for all packages needed by the web build
COPY apps/web/ apps/web/
COPY packages/editor/ packages/editor/
COPY packages/types-app/ packages/types-app/
COPY packages/types-campaign/ packages/types-campaign/
COPY tooling/tsconfig/ tooling/tsconfig/

# Vite bakes these into the bundle at build time.
# VITE_BASE_PATH sets Vite's `base` (asset URL prefix + import.meta.env.BASE_URL).
# Default /app/ matches prod's path-based layout under familiar.systems/app/.
# Preview builds override to /pr-N/app/ via the deploy workflow.
ARG VITE_HANKO_API_URL
ARG VITE_BASE_PATH=/app/
ENV VITE_HANKO_API_URL=${VITE_HANKO_API_URL}
ENV VITE_BASE_PATH=${VITE_BASE_PATH}

RUN pnpm --filter @familiar-systems/web build

# -- Stage 2: Serve -----------------------------------------------------------
FROM nginx:1.27-alpine

COPY apps/web/nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=builder /build/apps/web/dist /usr/share/nginx/html

EXPOSE 80
