.PHONY: all run clean setup

all: run

setup:
	rustup target add x86_64-unknown-none

run:
	cargo run --bin builder

clean:
	cargo clean
