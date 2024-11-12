# a Debian build container

FROM debian:bullseye-slim AS builder

RUN apt-get update && apt-get install -y \
    curl \
    build-essential \
    pkg-config \
    libssl-dev \
    && apt-get clean

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | \
    sh -s -- -y --default-toolchain nightly

ENV PATH="/root/.cargo/bin:${PATH}"

COPY . /app
WORKDIR /app

RUN cargo build --release

# copy the binary to a new image
FROM debian:bullseye-slim

RUN apt-get update && apt-get install -y \
    && apt-get clean

# Create a non-root user
RUN useradd -m -d /home/qbitun qbitun

# Copy the binary, rename it to 'app', and change ownership to the non-root user
COPY --from=builder /app/target/release/qbitun /home/qbitun/app
RUN chown qbitun:qbitun /home/qbitun/app

# Switch to the non-root user
USER qbitun

WORKDIR /home/qbitun

CMD ["./app"]