# CodeScope build system
# Automatically detects hardware acceleration: CUDA > Metal+Accelerate > CPU

UNAME := $(shell uname)
CUDA_BIN := $(firstword $(wildcard /usr/local/cuda/bin /usr/local/cuda-*/bin /opt/cuda/bin))
ifneq ($(CUDA_BIN),)
  export PATH := $(CUDA_BIN):$(PATH)
  FEATURES := --features semantic,cuda
  $(info [CUDA] Found at $(CUDA_BIN) — GPU embedding enabled)
else ifdef CUDA_PATH
  export PATH := $(CUDA_PATH)/bin:$(PATH)
  FEATURES := --features semantic,cuda
  $(info [CUDA] Found via CUDA_PATH — GPU embedding enabled)
else ifeq ($(UNAME),Darwin)
  FEATURES := --features accelerate
  $(info [macOS] Accelerate framework enabled)
else
  FEATURES :=
  $(info [CPU] No acceleration detected)
endif

SERVER := --manifest-path server/Cargo.toml
SETUP  := --manifest-path src-tauri/Cargo.toml

.PHONY: build install release dev clean check test setup search

# Build debug binary
build:
	cargo build $(SERVER) $(FEATURES)

# Install to ~/.local/bin
install:
	cargo install --path server $(FEATURES)

# Build optimized release binary
release:
	cargo build $(SERVER) --release $(FEATURES)

# Run dev server
dev:
	cargo run $(SERVER) $(FEATURES) -- web

# Run setup wizard (Tauri dev mode with hot reload)
setup: node_modules
	npx tauri dev

# Run search window (Tauri dev mode with hot reload)
search: node_modules
	npx tauri dev -- -- --search

# Type check everything
check: node_modules
	cargo check $(SERVER) $(FEATURES)
	cargo check $(SETUP)
	npx tsc --noEmit

# Run tests
test:
	cargo test $(SERVER) $(FEATURES)

# Install node dependencies if needed
node_modules: package.json
	npm install
	@touch $@

# Clean build artifacts
clean:
	cargo clean $(SERVER)
	cargo clean $(SETUP)
	rm -rf dist-setup
