# Batch Copy Examples

* `basic.rs` is the bare minimum application.
* `configuration.rs` demonstrates the config options and some discussion on tuning them for different workloads.
* `load_csv.rs` is a practical ETL application, loading a CSV of spot data prices from the AWS EC2 Spot API.
* `multi_producer.rs` shows thousands of concurrent tokio tasks at work, pushing data to a common table.

Run these examples using the Makefile in the root of this repo: `make test-examples`