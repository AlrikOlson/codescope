# CodeScope build system
# Automatically detects CUDA and enables GPU support

CUDA_BIN := $(firstword $(wildcard /usr/local/cuda/bin /usr/local/cuda-*/bin /opt/cuda/bin))
ifneq ($(CUDA_BIN),)
  export PATH := $(CUDA_BIN):$(PATH)
  FEATURES := --all-features
  $(info [CUDA] Found at $(CUDA_BIN) — GPU embedding enabled)
else ifdef CUDA_PATH
  export PATH := $(CUDA_PATH)/bin:$(PATH)
  FEATURES := --all-features
  $(info [CUDA] Found via CUDA_PATH — GPU embedding enabled)
else
  FEATURES :=
  $(info [CPU] No CUDA detected — building without GPU support)
endif

SERVER := --manifest-path server/Cargo.toml
SETUP  := --manifest-path src-tauri/Cargo.toml

.PHONY: build install release dev clean check test setup

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
setup:
	npx tauri dev $(if $(FEATURES),-- $(FEATURES))

# Type check everything
check:
	cargo check $(SERVER) $(FEATURES)
	cargo check $(SETUP)
	npx tsc --noEmit

# Run tests
test:
	cargo test $(SERVER) $(FEATURES)

# Clean build artifacts
clean:
	cargo clean $(SERVER)
	cargo clean $(SETUP)
	rm -rf dist-setup
