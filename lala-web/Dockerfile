# SPDX-License-Identifier: BSD-3-Clause
# Copyright (c) 2026 Aleksandr Ptakhin

# Stage 1: Build TypeScript with esbuild
FROM node:22-alpine AS build

WORKDIR /build
COPY package.json package-lock.json ./
RUN npm ci
COPY tsconfig.json ./
COPY src/ ./src/
RUN npm run build

# Stage 2: Serve with nginx
FROM nginx:alpine

# Copy static frontend files
COPY html/ /usr/share/nginx/html/

# Copy built JS bundles from build stage
COPY --from=build /build/html/js/ /usr/share/nginx/html/js/

# Copy the Nginx configuration
COPY nginx.conf /etc/nginx/conf.d/default.conf

# Expose port 80
EXPOSE 80

# Health check
HEALTHCHECK --interval=10s --timeout=3s --retries=3 \
    CMD wget --no-verbose --tries=1 --spider http://localhost/health || exit 1

# Start Nginx
CMD ["nginx", "-g", "daemon off;"]
