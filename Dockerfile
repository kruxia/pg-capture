# Multi-stage Dockerfile for pg-capture
# Produces a minimal Debian-based image

# Stage 1: Build
FROM ghcr.io/kruxia/rust:1.88 AS builder

# Create app directory
WORKDIR /app

# Copy manifests
COPY Cargo.toml Cargo.lock ./

# Copy source
COPY src ./src

# Build release binary with optimizations
RUN echo "deb http://deb.debian.org/debian bookworm-backports main" >>/etc/apt/sources.list \
&& apt update \
&& apt install -y upx-ucl
RUN cargo build --release \
&& upx target/release/pg-capture

# Stage 2: Runtime
FROM ghcr.io/kruxia/debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    tzdata \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN groupadd -g 1000 pgrepkafka && \
    useradd -u 1000 -g pgrepkafka -m pgrepkafka

# Copy binary from builder
COPY --from=builder /app/target/release/pg-capture /usr/local/bin/pg-capture

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
CMD ["/usr/local/bin/pg-capture"]
# CMD ["--help"]

# Labels
LABEL org.opencontainers.image.title="pg-capture"
LABEL org.opencontainers.image.description="PostgreSQL to Kafka CDC replicator"
LABEL org.opencontainers.image.version="0.1.0"
LABEL org.opencontainers.image.authors="pg-capture contributors"
LABEL org.opencontainers.image.source="https://github.com/kruxia/pg-capture"
LABEL org.opencontainers.image.licenses="MPL-2.0"
