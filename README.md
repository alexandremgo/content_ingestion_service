# Content Ingestion Service

## Overview
👷 This project is WIP - and a playground project for myself.

The vision: a service to search contents from your documents (EPUB, PDF or any text files)

There are 2 big business logic flows:
- extracting the content from the user's documents, and save it in searchable ways
- enabling, for the user, a fast search in their documents given a query (using full-text search and/or semantic search for ex)

Having said this, the project is not usable currently.

## Tech

Current infra:
- [RabbitMQ](https://rabbitmq.com/) for the message queue
- [PostgreSQL](https://www.postgresql.org/) for the relational database
- [MinIO](https://github.com/minio/minio) for the S3-compatible object storage
- [Meilisearch](https://www.meilisearch.com/) for the full-text search
- [Qdrant](https://qdrant.tech/) for the vector database

## Roadmap

What has been done:
- [x] : REST gateway service to handle requests from the users: `rest_gateway`
- [x] : services to extract contents: `content_ingestion_worker` (name need to change)
- [x] : service to handle full-text search: `fulltext_search_service`
- [x] : service to handle semantic search: `embedding_worker` (name need to change)
- [x] : communication between services using a message broker (RabbitMQ): either messages representing queued jobs or RPC requests
- [x] : authentication based on JWT token

The current work:
- [ ] : Replace RabbitMQ by Kafka (for the queue job) and gRPC (for the RPC requests)
- [ ] : Implement a more Hexagonal/Clean architecture in Rust
- [ ] : A diagram explaining the new backend architecture
- [ ] : Re-work of the semantic search service
- [ ] : Improve the content extraction: better handle text encodings, enabling reading PDF with OCR (and not just with the PDF encoded content)

## Configuration

There are several environments depending on where/how you want to deploy the services and workers:
- `develop`: not containerized, locally on your machine
- `local`: containerized, locally on your machine
- `production`: containerized, in production

## Tests
### Integration tests
#### Triggering integration tests with logs

To run with different logs: (`sqlx` logs are a bit spammy, cutting them out to reduce noise)
```bash
RUST_LOG="sqlx=error,info" TEST_LOG=enabled cargo test <a_test> | bunyan
```

#### Databases for integration tests

For each test, a new database is created (to enforce isolation). 
The name of each database will be: `test_<%Y-%m-%d_%H-%M-%S>_<randomly generated UUID>`


## Learning Resources

- I have learnt a lot about REST backend system in Rust thanks to Luca Palmieri's book: [Zero To Production In Rust](https://www.zero2prod.com/)
