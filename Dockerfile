# Multi-stage Dockerfile for pg-replicate-kafka
# Produces a minimal image under 50MB

# Stage 1: Build
FROM rust:1.75-alpine AS builder

# Install build dependencies
RUN apk add --no-cache \
    musl-dev \
    openssl-dev \
    pkgconfig \
    gcc \
    g++ \
    make \
    cmake

# Create app directory
WORKDIR /app

# Copy manifests
COPY Cargo.toml Cargo.lock ./

# Copy source
COPY src ./src

# Build release binary with optimizations
RUN RUSTFLAGS='-C target-feature=+crt-static' \
    cargo build --release --target x86_64-unknown-linux-musl

# Stage 2: Runtime
FROM alpine:3.19

# Install runtime dependencies
RUN apk add --no-cache \
    ca-certificates \
    tzdata \
    && rm -rf /var/cache/apk/*

# Create non-root user
RUN addgroup -g 1000 pgrepkafka && \
    adduser -D -u 1000 -G pgrepkafka pgrepkafka

# Copy binary from builder
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/pg-replicate-kafka /usr/local/bin/pg-replicate-kafka

# Create checkpoint directory
RUN mkdir -p /var/lib/pg-replicate-kafka && \
    chown -R pgrepkafka:pgrepkafka /var/lib/pg-replicate-kafka

# Switch to non-root user
USER pgrepkafka

# Set working directory
WORKDIR /var/lib/pg-replicate-kafka

# Expose metrics port (for future use)
EXPOSE 9090

# Default command
ENTRYPOINT ["/usr/local/bin/pg-replicate-kafka"]
CMD ["--help"]

# Labels
LABEL org.opencontainers.image.title="pg-replicate-kafka"
LABEL org.opencontainers.image.description="PostgreSQL to Kafka CDC replicator"
LABEL org.opencontainers.image.version="0.1.0"
LABEL org.opencontainers.image.authors="pg-replicate-kafka contributors"
LABEL org.opencontainers.image.source="https://github.com/yourusername/pg-replicate-kafka"
LABEL org.opencontainers.image.licenses="MPL-2.0"
