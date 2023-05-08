##### Build step #####
FROM rust:1.68 as build

RUN apt-get update && \
  apt-get -y upgrade && \
  apt-get -y install libpq-dev

WORKDIR /app
COPY . /app/

RUN cargo build --release

##### Production step #####
FROM gcr.io/distroless/cc-debian11
COPY --from=build /app/target/release/content_ingestion_service /usr/local/bin/content_ingestion_service

EXPOSE 8080
CMD ["content_ingestion_service"]
