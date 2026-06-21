# syntax=docker/dockerfile:1
# The Sentinel web client (SvelteKit SPA, adapter-static) served by nginx. Build
# context is the repo root (matches the CI build-push context). The SPA auto-targets
# control.thesentinels.online at runtime (see src/lib/config.ts), so no build-time API
# env is needed.
FROM node:22-alpine AS build
WORKDIR /app
# The deploying commit SHA (CI passes github.sha) → baked into the build and shown in the
# UI footer so a new version is visible at a glance. Defaults to "dev" for local builds.
ARG VITE_COMMIT=dev
ENV VITE_COMMIT=$VITE_COMMIT
COPY sentinel-app/package.json sentinel-app/package-lock.json ./
RUN npm ci
COPY sentinel-app/ ./
RUN npm run build

FROM nginx:alpine
COPY deploy/app/nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=build /app/build /usr/share/nginx/html
ARG VITE_COMMIT=dev
RUN echo "{\"commit\":\"${VITE_COMMIT}\"}" > /usr/share/nginx/html/version.json
EXPOSE 80
