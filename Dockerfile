# Reproducible build+run environment for the Big Bird (SOSP 2026) artifact.
#
#   docker compose up --build      # builds this image, runs ./reproduce.sh
#
# The image pre-builds the Rust engine (release) and the Python env so that
# `docker run` goes straight into the experiments. The Criteo dataset,
# experiment outputs, and figures are bind-mounted from the host (see
# compose.yaml) so results land in ./figures on the host.

# Pinned to a concrete recent stable (edition 2024 needs rustc >= 1.85);
# matches rust-toolchain.toml.
FROM rust:1.97-bookworm

# System deps: git for the build, plus the usual C toolchain for native crates.
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates curl git build-essential pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# uv manages the Python interpreter (3.12) and all Python deps.
COPY --from=ghcr.io/astral-sh/uv:0.9.18 /uv /uvx /usr/local/bin/

# Keep all caches inside the image (world-writable) so the container can also be
# run as a non-root host user if desired (compose `user:`), without breaking
# cargo/uv writes at runtime.
ENV HOME=/artifact \
    CARGO_HOME=/usr/local/cargo \
    UV_CACHE_DIR=/artifact/.uv-cache \
    UV_PYTHON_INSTALL_DIR=/artifact/.uv-python

WORKDIR /artifact

# Copy the whole artifact (the pdslib submodule is included; large/generated
# dirs are excluded via .dockerignore).
COPY . .

# Pre-build the release binary and the Python venv (cached layers).
RUN cd bigbirdeval && cargo build --release
RUN uv sync

# Let an arbitrary runtime uid write target/, .venv, caches, etc.
RUN chmod -R a+rwX /artifact /usr/local/cargo

# Default: run the full reproduction pipeline.
CMD ["bash", "./reproduce.sh"]
