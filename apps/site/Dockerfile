# Stage 1: Build
FROM node:24-slim AS build
WORKDIR /app

# Enable corepack for pnpm
RUN corepack enable && corepack prepare pnpm@latest --activate

# Copy workspace structure for dependency caching
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml turbo.json ./
COPY apps/site/package.json apps/site/
COPY apps/api/package.json apps/api/
COPY apps/collab/package.json apps/collab/
COPY apps/web/package.json apps/web/
COPY apps/worker/package.json apps/worker/
COPY packages/domain/package.json packages/domain/
COPY packages/db/package.json packages/db/
COPY packages/auth/package.json packages/auth/
COPY packages/editor/package.json packages/editor/
COPY packages/ai/package.json packages/ai/
COPY packages/queue/package.json packages/queue/
COPY tooling/tsconfig/package.json tooling/tsconfig/
COPY tooling/oxlint/package.json tooling/oxlint/

RUN pnpm install --frozen-lockfile

# Copy source needed for site build
COPY apps/site/ apps/site/
COPY packages/ packages/
COPY tooling/ tooling/

RUN pnpm turbo build --filter=@loreweaver/site

# Stage 2: Serve
FROM nginx:alpine
COPY --from=build /app/apps/site/dist/ /usr/share/nginx/html/
COPY apps/site/nginx.conf /etc/nginx/conf.d/default.conf
EXPOSE 80
