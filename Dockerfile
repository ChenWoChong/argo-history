FROM --platform=${BUILDPLATFORM:-linux/amd64} rust:1.94-bookworm AS builder

WORKDIR /workspace

COPY Cargo.toml Cargo.lock* ./
COPY src ./src
COPY templates ./templates
COPY static ./static

RUN cargo build --release

FROM gcr.io/distroless/cc-debian12:nonroot

WORKDIR /app
COPY --from=builder /workspace/target/release/argo-history /app/argo-history
COPY static /app/static

EXPOSE 8080 9443
ENTRYPOINT ["/app/argo-history"]
