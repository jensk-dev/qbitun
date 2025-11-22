FROM rust:latest AS builder

RUN apt-get update && apt-get install -y \
    curl \
    build-essential \
    pkg-config \
    libssl-dev \
    && apt-get clean

COPY . /app
WORKDIR /app

RUN cargo build --release

FROM gcr.io/distroless/cc-debian13

COPY --from=builder /app/target/release/qbitun /home/qbitun/app

WORKDIR /home/qbitun

CMD ["./app"]