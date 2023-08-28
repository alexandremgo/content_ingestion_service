FROM lukemathwalker/cargo-chef:latest-rust-1.69.0 as chef
WORKDIR /app
# Installs the required system dependencies for our linking configuration
RUN apt update && apt install lld clang -y

##### Dependencies planner stage #####
FROM chef as planner
COPY . .
# Computes a lock-like file for our project
RUN cargo chef prepare --recipe-path recipe.json

##### Dependencies builder stage #####
FROM chef as builder
COPY --from=planner /app/recipe.json recipe.json
# Builds our project dependencies, not our application!
RUN cargo chef cook --release --recipe-path recipe.json
# Up to this point, if our dependency tree stays the same,
# all layers should be cached. 
COPY . .
# Builds our project
RUN cargo build --release --bin content_ingestion_worker

##### Runtime stage #####
FROM debian:bullseye-slim AS runtime
WORKDIR /app

# Installs OpenSSL - it is dynamically linked by some of our dependencies
# Installs ca-certificates - it is needed to verify TLS certificates
# when establishing HTTPS connections
RUN apt-get update -y \
  && apt-get install -y --no-install-recommends openssl ca-certificates \
  # Cleans up
  && apt-get autoremove -y \
  && apt-get clean -y \
  && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/content_ingestion_worker content_ingestion_worker
COPY configuration configuration
ENV APP_ENVIRONMENT local
ENTRYPOINT ["./content_ingestion_worker"]
