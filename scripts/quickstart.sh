#!/bin/bash
# Quick start script for pg-replicate-kafka

set -e

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

echo -e "${BLUE}"
echo "======================================"
echo "   pg-replicate-kafka Quick Start     "
echo "======================================"
echo -e "${NC}"

# Check for Docker
if ! command -v docker &> /dev/null; then
    echo -e "${RED}Error: Docker is not installed${NC}"
    echo "Please install Docker from https://docs.docker.com/get-docker/"
    exit 1
fi

# Check for Docker Compose
if ! command -v docker-compose &> /dev/null; then
    if ! docker compose version &> /dev/null; then
        echo -e "${RED}Error: Docker Compose is not installed${NC}"
        echo "Please install Docker Compose from https://docs.docker.com/compose/install/"
        exit 1
    fi
    # Use 'docker compose' instead of 'docker-compose'
    DOCKER_COMPOSE="docker compose"
else
    DOCKER_COMPOSE="docker-compose"
fi

echo -e "${GREEN}✓ Prerequisites checked${NC}"
echo ""

# Start services
echo -e "${YELLOW}Starting PostgreSQL and Kafka...${NC}"
$DOCKER_COMPOSE up -d postgres kafka

# Wait for services
echo -n "Waiting for services to be ready"
for i in {1..30}; do
    if $DOCKER_COMPOSE exec -T postgres pg_isready -U postgres &>/dev/null && \
       $DOCKER_COMPOSE exec -T kafka kafka-topics.sh --bootstrap-server localhost:9092 --list &>/dev/null 2>&1; then
        echo -e " ${GREEN}✓${NC}"
        break
    fi
    echo -n "."
    sleep 2
done

if [ $i -eq 30 ]; then
    echo -e " ${RED}✗${NC}"
    echo "Services failed to start. Check logs with: $DOCKER_COMPOSE logs"
    exit 1
fi

# Create test database and table
echo -e "${YELLOW}Setting up test database...${NC}"
$DOCKER_COMPOSE exec -T postgres psql -U postgres -d testdb <<EOF
-- Create test table
CREATE TABLE IF NOT EXISTS users (
    id SERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    email TEXT NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Create publication
CREATE PUBLICATION IF NOT EXISTS my_publication FOR TABLE users;
EOF

echo -e "${GREEN}✓ Database setup complete${NC}"

# Build and start pg-replicate-kafka
echo -e "${YELLOW}Building pg-replicate-kafka...${NC}"
$DOCKER_COMPOSE build pg-replicate-kafka

echo -e "${YELLOW}Starting pg-replicate-kafka...${NC}"
$DOCKER_COMPOSE up -d pg-replicate-kafka

# Wait for replicator to be ready
sleep 5

# Insert test data
echo -e "${YELLOW}Inserting test data...${NC}"
$DOCKER_COMPOSE exec -T postgres psql -U postgres -d testdb <<EOF
INSERT INTO users (name, email) VALUES 
    ('Alice Johnson', 'alice@example.com'),
    ('Bob Smith', 'bob@example.com'),
    ('Charlie Brown', 'charlie@example.com');

UPDATE users SET email = 'alice.johnson@example.com' WHERE name = 'Alice Johnson';

DELETE FROM users WHERE name = 'Charlie Brown';
EOF

echo -e "${GREEN}✓ Test data inserted${NC}"

# Show results
echo ""
echo -e "${BLUE}=== Viewing Replicated Messages ===${NC}"
echo ""

echo "Waiting for messages to be replicated..."
sleep 2

echo -e "${YELLOW}Messages in Kafka topic 'cdc.public.users':${NC}"
$DOCKER_COMPOSE exec -T kafka kafka-console-consumer.sh \
    --bootstrap-server localhost:9092 \
    --topic cdc.public.users \
    --from-beginning \
    --max-messages 5 \
    --formatter kafka.tools.DefaultMessageFormatter \
    --property print.timestamp=true \
    --property print.key=true \
    --property print.value=true \
    --timeout-ms 5000 2>/dev/null || true

echo ""
echo -e "${GREEN}✓ Quick start complete!${NC}"
echo ""
echo -e "${BLUE}Next steps:${NC}"
echo "1. View logs: $DOCKER_COMPOSE logs -f pg-replicate-kafka"
echo "2. Insert more data: $DOCKER_COMPOSE exec postgres psql -U postgres -d testdb"
echo "3. Monitor Kafka: $DOCKER_COMPOSE --profile monitoring up -d kafka-ui"
echo "   Then open http://localhost:8080"
echo "4. Stop services: $DOCKER_COMPOSE down"
echo ""
echo -e "${YELLOW}Configuration:${NC}"
echo "- Environment variables: see docker-compose.yml"
echo "- Checkpoint: stored in Docker volume"
echo ""
echo -e "${YELLOW}Testing custom configuration:${NC}"
echo "1. Edit environment variables in docker-compose.yml"
echo "2. Restart: $DOCKER_COMPOSE restart pg-replicate-kafka"
echo ""