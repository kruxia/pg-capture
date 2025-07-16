# Multi-stage Dockerfile for pg-capture
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
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/pg-capture /usr/local/bin/pg-capture

# Create checkpoint directory
RUN mkdir -p /var/lib/pg-capture && \
    chown -R pgrepkafka:pgrepkafka /var/lib/pg-capture

# Switch to non-root user
USER pgrepkafka

# Set working directory
WORKDIR /var/lib/pg-capture

# Expose metrics port (for future use)
EXPOSE 9090

# Default command
ENTRYPOINT ["/usr/local/bin/pg-capture"]
CMD ["--help"]

# Labels
LABEL org.opencontainers.image.title="pg-capture"
LABEL org.opencontainers.image.description="PostgreSQL to Kafka CDC replicator"
LABEL org.opencontainers.image.version="0.1.0"
LABEL org.opencontainers.image.authors="pg-capture contributors"
LABEL org.opencontainers.image.source="https://github.com/yourusername/pg-capture"
LABEL org.opencontainers.image.licenses="MPL-2.0"
