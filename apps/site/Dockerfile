# Multi-stage build for the Astro static site.
# Build context must be the repo root:
#   docker build -f apps/site/Dockerfile -t loreweaver-site .

# -- Stage 1: Build ----------------------------------------------------------
FROM node:24-slim AS build

ENV PNPM_HOME="/pnpm"
ENV PATH="$PNPM_HOME:$PATH"

RUN npm install -g pnpm@10.32.1

WORKDIR /app

# Copy workspace config + lockfile first (layer caching)
COPY pnpm-workspace.yaml pnpm-lock.yaml package.json ./

# Copy only the site's package.json (no other apps/packages needed at install)
COPY apps/site/package.json apps/site/package.json

# Install deps for site only (frozen lockfile for reproducibility)
RUN pnpm install --frozen-lockfile --filter @loreweaver/site...

# Copy site source
COPY apps/site/ apps/site/

# Build static site + pagefind index
ARG SITE_URL=https://preview.loreweaver.no
ENV SITE_URL=$SITE_URL
RUN pnpm --filter @loreweaver/site build

# -- Stage 2: Serve -----------------------------------------------------------
FROM nginx:alpine

COPY --from=build /app/apps/site/dist/ /usr/share/nginx/html/

EXPOSE 80
