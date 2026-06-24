# syntax=docker/dockerfile:1
# The Sentinel marketing landing — a SvelteKit static site (prerendered) served by
# nginx at the apex thesentinels.online. Build context is the repo root (matches CI).
FROM node:22-alpine AS build
WORKDIR /app
COPY landing/package.json landing/package-lock.json ./
RUN npm ci
COPY landing/ ./
COPY styles/ /styles/
RUN npm run build

FROM nginx:alpine
COPY deploy/landing/nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=build /app/build /usr/share/nginx/html
EXPOSE 80
