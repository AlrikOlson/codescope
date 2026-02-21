FROM rust:1.93-alpine AS builder
RUN apk add --no-cache musl-dev perl

WORKDIR /app

# Copy manifests first — cached until Cargo.toml/Cargo.lock change
COPY server/Cargo.toml server/Cargo.lock server/

# Build dependencies with a dummy main to cache the dep layer
RUN mkdir -p server/src && echo 'fn main() {}' > server/src/main.rs
RUN cargo build --release --manifest-path server/Cargo.toml --features semantic 2>/dev/null; \
    rm -rf server/src

# Copy real source and build
COPY server/src/ server/src/
ARG VERSION=""
RUN if [ -n "$VERSION" ]; then \
      perl -i -pe "s/^version = \".*\"/version = \"$VERSION\"/" server/Cargo.toml; \
    fi
RUN touch server/src/main.rs && \
    cargo build --release --manifest-path server/Cargo.toml --features semantic

FROM scratch AS output
COPY --from=builder /app/server/target/release/codescope /codescope
