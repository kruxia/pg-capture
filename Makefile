.PHONY: help build test run docker-build docker-run clean dev test-integration

# Default target
help:
	@echo "Available targets:"
	@echo "  make build          - Build the project"
	@echo "  make test           - Run unit tests"
	@echo "  make test-integration - Run integration tests with Docker"
	@echo "  make run            - Run pg-capture (requires env vars)"
	@echo "  make dev            - Start development environment with Docker Compose"
	@echo "  make docker-build   - Build Docker image"
	@echo "  make clean          - Clean build artifacts"
	@echo "  make fmt            - Format code"
	@echo "  make lint           - Run clippy linter"

# Build the project
build:
	cargo build --release

# Run unit tests
test:
	cargo test --lib --bins

# Run integration tests with Docker
test-integration:
	docker-compose -f docker-compose.test.yml up --build --abort-on-container-exit --exit-code-from test-runner

# Run pg-capture (requires environment variables)
run:
	@if [ -z "$$PG_DATABASE" ] || [ -z "$$PG_USERNAME" ] || [ -z "$$PG_PASSWORD" ] || [ -z "$$KAFKA_BROKERS" ]; then \
		echo "Error: Required environment variables not set"; \
		echo "Required: PG_DATABASE, PG_USERNAME, PG_PASSWORD, KAFKA_BROKERS"; \
		echo "See .envrc.example for all options"; \
		exit 1; \
	fi
	cargo run --release

# Start development environment
dev:
	docker-compose up -d
	@echo "Services started:"
	@echo "  PostgreSQL: localhost:5432"
	@echo "  Kafka: localhost:9092"
	@echo "  Kafka UI: http://localhost:8080"
	@echo ""
	@echo "To start pg-capture:"
	@echo "  docker-compose --profile app up -d"

# Build Docker image
docker-build:
	docker build -t pg-capture:latest .

# Run with Docker
docker-run:
	@if [ -z "$$PG_DATABASE" ] || [ -z "$$PG_USERNAME" ] || [ -z "$$PG_PASSWORD" ] || [ -z "$$KAFKA_BROKERS" ]; then \
		echo "Error: Required environment variables not set"; \
		echo "Required: PG_DATABASE, PG_USERNAME, PG_PASSWORD, KAFKA_BROKERS"; \
		exit 1; \
	fi
	docker run --rm \
		-e PG_DATABASE \
		-e PG_USERNAME \
		-e PG_PASSWORD \
		-e KAFKA_BROKERS \
		-e PG_HOST=$${PG_HOST:-localhost} \
		-e KAFKA_TOPIC_PREFIX=$${KAFKA_TOPIC_PREFIX:-cdc} \
		-v pg-capture-data:/var/lib/pg-capture \
		pg-capture:latest

# Clean build artifacts
clean:
	cargo clean
	rm -f checkpoint.json
	docker-compose down -v

# Format code
fmt:
	cargo fmt

# Run linter
lint:
	cargo clippy -- -D warnings

# Check everything before committing
check: fmt lint test
	@echo "All checks passed!"