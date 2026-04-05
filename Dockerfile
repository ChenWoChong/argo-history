FROM rust:1.94-bookworm

WORKDIR /app
COPY target-linux/release/argo-history /app/argo-history
COPY static /app/static

EXPOSE 8080 9443
ENTRYPOINT ["/app/argo-history"]
