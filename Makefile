BIN := target/release/audiobook-splitter-rust
MODEL := models/ggml-base.en.bin

.PHONY: dev build run test cli model

dev: $(MODEL)    ## Debug build, opens the GUI
	cargo run

build:   ## Optimized single binary at $(BIN)
	cargo build --release

run: build $(MODEL) ## Build and launch the release GUI
	./$(BIN)

test:
	cargo test

cli: $(MODEL)    ## Original interactive terminal flow
	cargo run -- --cli

model: $(MODEL) ## Download the Whisper speech-to-text model (~148 MB, once)

$(MODEL):
	mkdir -p models
	curl -L --fail -o $@.part https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin
	mv $@.part $@
