.PHONY: build check test clippy clean

build:
	cargo build --release

check: clippy test

clippy:
	cargo clippy -- -D warnings

test:
	cargo test

clean:
	cargo clean
