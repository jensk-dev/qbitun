FROM rust:latest AS builder

RUN apt-get update && apt-get install -y \
    curl \
    build-essential \
    pkg-config \
    libssl-dev \
    libzstd1 \
    && apt-get clean

COPY . /app
WORKDIR /app

RUN cargo build --release

FROM gcr.io/distroless/cc-debian13

COPY --from=builder /app/target/release/qbitun /home/qbitun/app
COPY --from=builder /usr/lib/x86_64-linux-gnu/libzstd.so* /usr/lib/x86_64-linux-gnu/

WORKDIR /home/qbitun

CMD ["./app"]