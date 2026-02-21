# CodeScope build system
# Automatically detects hardware acceleration: CUDA > Metal+Accelerate > CPU

# Cross-platform OS detection
ifeq ($(OS),Windows_NT)
  DETECTED_OS := Windows
else
  UNAME := $(shell uname)
  ifeq ($(UNAME),Darwin)
    DETECTED_OS := macOS
  else
    DETECTED_OS := Linux
  endif
endif

# Hardware acceleration detection
# CUDA_PATH is set exclusively by the NVIDIA CUDA Toolkit installer (not the
# driver-only install), so its presence is authoritative proof that nvcc exists.
# We trust it directly because Make's wildcard cannot handle the spaces in
# "C:\Program Files\..." that CUDA_PATH typically contains.
ifdef CUDA_PATH
  CUDA_BIN := $(subst \,/,$(CUDA_PATH))/bin
  NVCC_EXISTS := yes
else ifeq ($(DETECTED_OS),Windows)
  # Fallback: scan default install location (escaped spaces for wildcard)
  CUDA_BIN := $(firstword $(wildcard C:/Program\ Files/NVIDIA\ GPU\ Computing\ Toolkit/CUDA/v*/bin))
else
  # Linux/other default install locations
  CUDA_BIN := $(firstword $(wildcard /usr/local/cuda/bin /usr/local/cuda-*/bin /opt/cuda/bin))
endif

# When CUDA_PATH wasn't set, verify nvcc in the discovered directory
ifndef NVCC_EXISTS
  ifneq ($(CUDA_BIN),)
    ifeq ($(DETECTED_OS),Windows)
      NVCC_EXISTS := $(wildcard $(CUDA_BIN)/nvcc.exe)
    else
      NVCC_EXISTS := $(wildcard $(CUDA_BIN)/nvcc)
    endif
  endif
endif

# MSVC wrapper — on Windows with CUDA, wraps build commands so nvcc can find
# cl.exe (the MSVC host compiler) and gets the correct CRT flags (/MD).
# The wrapper uses vswhere.exe to locate Visual Studio, avoiding all the
# Make-can't-handle-spaces-in-paths issues.
CUDA_WRAP :=

ifneq ($(NVCC_EXISTS),)
  ACCEL_FEATURES := cuda
  ifeq ($(DETECTED_OS),Windows)
    export PATH := $(CUDA_BIN);$(PATH)
    CUDA_WRAP := scripts\with-msvc.cmd
  else
    export PATH := $(CUDA_BIN):$(PATH)
  endif
  $(info [CUDA] Found nvcc at $(CUDA_BIN) — GPU embedding enabled)
else
  ifneq ($(CUDA_BIN),)
    $(info [CUDA] Directory found but nvcc not present — install CUDA toolkit for GPU support)
  endif
  ifeq ($(DETECTED_OS),macOS)
    ACCEL_FEATURES := accelerate
    $(info [macOS] Accelerate framework enabled)
  else
    ACCEL_FEATURES :=
    ifeq ($(CUDA_BIN),)
      $(info [CPU] No acceleration detected — install CUDA toolkit for GPU support)
    endif
  endif
endif

# Common features always enabled
BASE_FEATURES := treesitter,semantic

# Combine: base + hardware acceleration
ifneq ($(ACCEL_FEATURES),)
  ALL_FEATURES := $(BASE_FEATURES),$(ACCEL_FEATURES)
else
  ALL_FEATURES := $(BASE_FEATURES)
endif

FEATURES := --features $(ALL_FEATURES)

SERVER := --manifest-path server/Cargo.toml
SETUP  := --manifest-path src-tauri/Cargo.toml

# npm install writes this file, so we use it as a stamp to track freshness
NODE_STAMP := node_modules/.package-lock.json

.PHONY: build install release dev clean check test setup search

# Build debug binary
build:
	$(CUDA_WRAP) cargo build $(SERVER) $(FEATURES)

# Install to ~/.local/bin
install:
	$(CUDA_WRAP) cargo install --path server $(FEATURES)

# Build optimized release binary
release:
	$(CUDA_WRAP) cargo build $(SERVER) --release $(FEATURES)

# Run dev server
dev:
	$(CUDA_WRAP) cargo run $(SERVER) $(FEATURES) -- web

# Run setup wizard (Tauri dev mode with hot reload)
setup: $(NODE_STAMP)
	$(CUDA_WRAP) npx tauri dev --features $(ALL_FEATURES)

# Run search window (Tauri dev mode with hot reload)
search: $(NODE_STAMP)
	$(CUDA_WRAP) npx tauri dev --features $(ALL_FEATURES) -- -- --search

# Type check everything
check: $(NODE_STAMP)
	$(CUDA_WRAP) cargo check $(SERVER) $(FEATURES)
	$(CUDA_WRAP) cargo check $(SETUP) $(FEATURES)
	npx tsc --noEmit

# Run tests
test:
	$(CUDA_WRAP) cargo test $(SERVER) $(FEATURES)

# Install node dependencies if needed
$(NODE_STAMP): package.json
	npm install

# Clean build artifacts
clean:
	cargo clean $(SERVER)
	cargo clean $(SETUP)
ifeq ($(DETECTED_OS),Windows)
	if exist dist-setup rmdir /s /q dist-setup
else
	rm -rf dist-setup
endif
