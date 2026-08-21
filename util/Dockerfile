FROM rust:1-bookworm AS rust-builder
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY redisdb/ redisdb/
RUN cargo build --release --bin redisops

FROM gcc:14-bookworm AS cpp-builder
RUN apt-get update && apt-get install -y cmake libseccomp-dev && rm -rf /var/lib/apt/lists/*
WORKDIR /src/sandbox
COPY sandbox/ .
RUN mkdir -p build && cd build && cmake .. && make -j$(nproc)

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=rust-builder /src/target/release/redisops ./
COPY --from=cpp-builder /src/sandbox/build/sandbox ./
EXPOSE 1234 9090
CMD ["sh", "-c", "./redisops & ./sandbox --host 127.0.0.1 --port 1234 --type local --metrics-port 9090 && wait"]
