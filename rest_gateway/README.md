# Run
## Health check

```bash
curl http://127.0.0.1:4242/health_check -v
```

# Tests
## Run integration tests

To display tracing events/spans (`TEST_LOG` set on the integration tests) of the functions under tests:
```bash
RUST_LOG="sqlx=error,debug" TEST_LOG=true cargo test health_check | bunyan
```
## Run unit tests

To run tests and display prints of the under-test function:
```bash
RUST_LOG=debug cargo test test_xml_extract_content -- --nocapture
```
