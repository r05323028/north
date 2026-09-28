FROM node:22-bookworm-slim

ARG VERSION
ARG SOURCE_COMMIT
ARG SOURCE_URL
LABEL org.opencontainers.image.title="North web" \
      org.opencontainers.image.version="$VERSION" \
      org.opencontainers.image.revision="$SOURCE_COMMIT" \
      org.opencontainers.image.source="$SOURCE_URL"

ENV NODE_ENV=production \
    NEXT_TELEMETRY_DISABLED=1 \
    HOSTNAME=0.0.0.0 \
    PORT=3000
WORKDIR /app
COPY --chown=node:node ./ ./
USER node
EXPOSE 3000
CMD ["node", "server.js"]
