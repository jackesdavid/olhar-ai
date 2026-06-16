# syntax=docker/dockerfile:1
# The Sentinel web client (SvelteKit SPA, adapter-static) served by nginx. Build
# context is the repo root (matches the CI build-push context). The SPA auto-targets
# control.thesentinels.online at runtime (see src/lib/config.ts), so no build-time API
# env is needed.
FROM node:22-alpine AS build
WORKDIR /app
COPY sentinel-app/package.json sentinel-app/package-lock.json ./
RUN npm ci
COPY sentinel-app/ ./
RUN npm run build

FROM nginx:alpine
COPY deploy/app/nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=build /app/build /usr/share/nginx/html
EXPOSE 80
