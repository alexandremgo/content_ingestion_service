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

# Checks if a custom user has been set, otherwise default to 'postgres'
export DB_USER=${POSTGRES_USER:=postgres}
# Checks if a custom password has been set, otherwise default to 'password'
export DB_PASSWORD="${POSTGRES_PASSWORD:=password}"
# Checks if a custom database name has been set, otherwise default to 'newsletter'
export DB_NAME="${POSTGRES_DB:=newsletter}"
# Checks if a custom port has been set, otherwise default to '5432'
export DB_PORT="${POSTGRES_PORT:=5432}"

# Allow to skip Docker if a containers are already running
if [[ -z "${SKIP_DOCKER}" ]]
then

  if [[ -n "${BUILD_CONTAINERS}" ]]
  then
    docker-compose build
    >&2 echo "🏗️ Containers were built successfully"
  fi

  docker-compose up
  >&2 echo "🚚 Containers are up"
fi

# Keeps pinging Postgres until it's ready to accept commands
export PGPASSWORD="${DB_PASSWORD}"
until psql -h "localhost" -U "${DB_USER}" -p "${DB_PORT}" -d "postgres" -c '\q'; do
  >&2 echo "🛌 Postgres is still unavailable - sleeping"
  sleep 1
done

>&2 echo "🎉 Postgres is up and running on port ${DB_PORT}!"

export DATABASE_URL=postgres://${DB_USER}:${DB_PASSWORD}@localhost:${DB_PORT}/${DB_NAME}
sqlx database create

sqlx migrate run
>&2 echo "🏭 Postgres has been migrated, ready to go!"