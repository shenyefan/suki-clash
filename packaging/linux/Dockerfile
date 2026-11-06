ARG RUST_VERSION=1.99.1
FROM rust:${RUST_VERSION}-slim-bookworm AS rust
FROM ghcr.io/astral-sh/uv:0.13.7 AS uv
FROM mcr.microsoft.com/dotnet/sdk:8.0-jammy

COPY --from=rust /usr/local/rustup /opt/rustup
COPY --from=rust /usr/local/cargo /opt/cargo
COPY --from=uv /uv /usr/local/bin/uv

ENV RUSTUP_HOME=/opt/rustup \
    CARGO_HOME=/opt/cargo \
    UV_PYTHON_INSTALL_DIR=/opt/python \
    UV_PYTHON_BIN_DIR=/usr/local/bin \
    PATH=/opt/cargo/bin:/opt/velopack:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
    CARGO_BUILD_JOBS=2 \
    CARGO_INCREMENTAL=0 \
    CARGO_PROFILE_RELEASE_DEBUG=0 \
    DOTNET_CLI_TELEMETRY_OPTOUT=1

RUN sed -i 's|http://|https://|g' /etc/apt/sources.list \
    && apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
       build-essential clang cmake ninja-build pkg-config ca-certificates \
       libfontconfig1-dev libfreetype6-dev libxkbcommon-dev libxkbcommon-x11-dev \
       libxcb1-dev libxcb-xkb-dev libwayland-dev libssl-dev libasound2-dev \
       libudev-dev libvulkan-dev libgit2-dev patchelf file libfuse2 squashfs-tools desktop-file-utils \
    && rm -rf /var/lib/apt/lists/*
ARG VELOPACK_VERSION=1.2.161
RUN uv python install 3.12 --default \
    && dotnet tool install vpk --version "$VELOPACK_VERSION" --tool-path /opt/velopack

WORKDIR /workspace
