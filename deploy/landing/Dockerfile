# syntax=docker/dockerfile:1
# The Sentinel marketing landing page (static, single file) served by nginx at the
# apex thesentinels.online. Build context is the repo root (matches CI).
FROM nginx:alpine
COPY deploy/landing/nginx.conf /etc/nginx/conf.d/default.conf
COPY deploy/landing/index.html /usr/share/nginx/html/index.html
EXPOSE 80
