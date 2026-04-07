FROM debian:bookworm-slim

WORKDIR /app

# Install only runtime dependencies
# ca-certificates: for HTTPS requests
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY target-linux/release/argo-history /app/argo-history
COPY static /app/static

EXPOSE 8080 9443
ENTRYPOINT ["/app/argo-history"]