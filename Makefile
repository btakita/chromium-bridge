.PHONY: build release check test clippy install publish clean

build:
	cargo build --release

release: check
	cargo build --release

check: clippy test

clippy:
	cargo clippy -- -D warnings

test:
	cargo test

install:
	cargo install --path .

publish: check
	cargo publish

clean:
	cargo clean
