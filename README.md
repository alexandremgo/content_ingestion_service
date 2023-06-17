# Content Ingestion Service

Services:
- [MinIO](https://github.com/minio/minio) for the S3-compatible object storage
- [RabbitMQ](https://kafka.apache.org/) for the message queue
- [PostgreSQL](https://www.postgresql.org/) for the relational database

# Tests
## Integration tests
### Triggering integration tests with logs

To run with different logs: (`sqlx` logs are a bit spammy, cutting them out to reduce noise)
```bash
RUST_LOG="sqlx=error,info" TEST_LOG=enabled cargo test <a_test> | bunyan
```

### Databases for integration tests

For each test, a new database is created (to enforce isolation). 
The name of each database will be: `test_<%Y-%m-%d_%H-%M-%S>_<randomly generated UUID>`
