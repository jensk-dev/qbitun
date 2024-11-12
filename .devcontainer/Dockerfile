FROM debian:bullseye-slim AS builder

RUN apt-get update && apt-get install -y \
    curl \
    && apt-get clean

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | \
    sh -s -- -y --default-toolchain nightly

FROM debian:bullseye-slim

RUN apt-get update && apt-get install -y \
    git \
    gnupg2 \
    build-essential \
    pkg-config \
    libssl-dev \
    && apt-get clean

# Copy Rust toolchain from the builder stage
COPY --from=builder /root/.cargo /root/.cargo
COPY --from=builder /root/.rustup /root/.rustup

# Set PATH for Rust
ENV PATH="/root/.cargo/bin:${PATH}"

# Confirm installation by showing the Rust version
RUN rustc --version