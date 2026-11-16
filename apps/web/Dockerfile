# syntax=docker/dockerfile:1
# supellex — image du front Next.js (BFF). Contexte de build : racine du dépôt
# (workspace pnpm : apps/web + packages/types + pnpm-lock.yaml).
# Image épinglée sur la LTS Node du moment (voir .nvmrc), pnpm dernière stable.
FROM node:26.13.1-alpine AS build
WORKDIR /app
RUN npm install -g pnpm@12.21.0
COPY package.json pnpm-workspace.yaml pnpm-lock.yaml ./
COPY apps/web/package.json apps/web/package.json
COPY packages/types/package.json packages/types/package.json
RUN pnpm install --frozen-lockfile
COPY apps/web apps/web
COPY packages/types packages/types
RUN pnpm --filter supellex-web build

FROM node:26.13.1-alpine
ENV NODE_ENV=production \
    PORT=3000
WORKDIR /app
RUN npm install -g pnpm@12.21.0
COPY --from=build /app /app
# exécution sans root (user node de l'image) ; Next écrit .next/cache
RUN chown -R node:node /app
USER node
EXPOSE 3000
HEALTHCHECK --interval=10s --timeout=5s --retries=6 --start-period=20s \
  CMD node -e "fetch('http://localhost:3000/login').then(r=>process.exit(r.ok?0:1)).catch(()=>process.exit(1))"
CMD ["pnpm", "--filter", "supellex-web", "start"]
