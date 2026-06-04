FROM rust:1.91-bookworm AS builder
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*

ENV CARGO_NET_RETRY=10
ENV CARGO_TERM_COLOR=always

COPY Cargo.toml Cargo.lock* ./
RUN mkdir -p src && echo 'fn main() {}' > src/main.rs && \
    cargo build --release && \
    rm -rf src

COPY src ./src
COPY templates ./templates
COPY static ./static
RUN touch src/main.rs && cargo build --release

FROM gcr.io/distroless/cc-debian12 AS runtime
WORKDIR /app
COPY --from=builder /app/target/release/s3-explorer /s3-explorer
COPY --from=builder /app/templates /app/templates
COPY --from=builder /app/static /app/static
ENV PORT=3000
ENV RUST_LOG=info,s3_explorer=info,tower_http=info
EXPOSE 3000
USER nonroot:nonroot
ENTRYPOINT ["/s3-explorer"]
