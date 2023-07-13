#!/usr/bin/env bash
set -x
# Exits immediately with the status error of any command  
# that fails or returns a non-zero status (also inside a pipepine)
set -eo pipefail

# Checks script dependencies
if ! [ -x "$(command -v psql)" ]; then
  echo >&2 "❌ Error: psql is not installed."
  exit 1
fi

if ! [ -x "$(command -v sqlx)" ]; then
  echo >&2 "❌ Error: sqlx is not installed."
  echo >&2 "Use:"
  echo >&2 "    cargo install --version=0.5.7 sqlx-cli --no-default-features --features postgres"
  echo >&2 "or:"
  echo >&2 "    cargo install sqlx-cli --no-default-features --features postgres"
  echo >&2 "to install it."
  exit 1
fi

# Posgres env variables:
# Checks if a custom user has been set, otherwise default to 'postgres'
export DB_USER=${POSTGRES_USER:=postgres}
# Checks if a custom password has been set, otherwise default to 'password'
export DB_PASSWORD="${POSTGRES_PASSWORD:=password}"
# Checks if a custom database name has been set, otherwise default to 'newsletter'
export DB_NAME="${POSTGRES_DB:=content_ingestion}"
# Checks if a custom port has been set, otherwise default to '5432'
export DB_PORT="${POSTGRES_PORT:=5432}"

# MinIO env variables:
export OBJECT_STORAGE_USER=${MINIO_USER:=minio}
export OBJECT_STORAGE_PASSWORD="${MINIO_PASSWORD:=password}"
export OBJECT_STORAGE_PORT="${MINIO_PORT:=9000}"
export OBJECT_STORAGE_ADMIN_PORT="${MINIO_ADMIN_PORT:=9001}"
export OBJECT_STORAGE_SITE_REGION="${MINIO_SITE_REGION:=eu-fr-1}"
export OBJECT_STORAGE_SITE_NAME="${MINIO_SITE_NAME:=par-rack-1}"

# RabbitMQ env variables:
export RABBITMQ_DEFAULT_USER=${RABBITMQ_DEFAULT_USER:=guest}
export RABBITMQ_DEFAULT_PASS=${RABBITMQ_DEFAULT_PASS:=guest}

# Meilisearch env variables
export MEILI_PORT=${MEILI_PORT:=7700}
export MEILI_MASTER_KEY=${MEILI_MASTER_KEY:=masterkey}
export MEILI_NO_ANALYTICS=${MEILI_NO_ANALYTICS:=true}
# Not scalable if several Meilisearch indexes. We might create "migration" using an empty dump file.
export MEILI_EXTRACTED_CONTENT_INDEX="extracted_contents"
export MEILI_EXTRACTED_CONTENT_PRIMARY_KEY="id"

# Allow to skip Docker if a containers are already running
if [[ -z "${SKIP_DOCKER}" ]]
then
  if [[ -n "${REMOVE_PREVIOUS_CONTAINERS}" ]]
  then
    docker-compose down
    >&2 echo "🧼 Containers were removed successfully"
  fi

  if [[ -n "${BUILD_CONTAINERS}" ]]
  then
    docker-compose build
    >&2 echo "🏗️ Containers were built successfully"
  fi

  docker-compose up -d
  >&2 echo "🚚 Containers are up"
fi


# Keeps pinging Meilisearch until it's ready to accept commands
until curl -X GET "http://localhost:${MEILI_PORT}" -H 'Content-Type: application/json' -H "Authorization: Bearer ${MEILI_MASTER_KEY}" >/dev/null 2>&1; do
  >&2 echo "🛌 Meilisearch is still unavailable - sleeping"
  sleep 1
done

# Sets up the Meilisearch indexes
echo "🛠️ Setting up the Meilisearch indexes"

# Creates the Meilisearch indexes
curl -X POST "http://localhost:${MEILI_PORT}/indexes" \
  -H 'Content-Type: application/json' \
  -H 'Authorization: Bearer masterkey' \
  --data-binary "{ \"uid\": \"${MEILI_EXTRACTED_CONTENT_INDEX}\", \"primaryKey\": \"${MEILI_EXTRACTED_CONTENT_PRIMARY_KEY}\" }"

# Keeps pinging Postgres until it's ready to accept commands
export PGPASSWORD="${DB_PASSWORD}"
until psql -h "localhost" -U "${DB_USER}" -p "${DB_PORT}" -d "postgres" -c '\q'; do
  >&2 echo "🛌 Postgres is still unavailable - sleeping"
  sleep 1
done

echo "🎉 Postgres is up and running on port ${DB_PORT}!"

# Necessary to work with sqlx cli and sqlx compile-time verification
export DATABASE_URL=postgres://${DB_USER}:${DB_PASSWORD}@localhost:${DB_PORT}/${DB_NAME}
sqlx database create

sqlx migrate run
echo "🏭 Postgres has been migrated, ready to go!"
