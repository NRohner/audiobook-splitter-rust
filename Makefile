BIN := target/release/audiobook-splitter-rust

.PHONY: dev build run test cli

dev:     ## Debug build, opens the GUI
	cargo run

build:   ## Optimized single binary at $(BIN)
	cargo build --release

run: build ## Build and launch the release GUI
	./$(BIN)

test:
	cargo test

cli:     ## Original interactive terminal flow
	cargo run -- --cli
