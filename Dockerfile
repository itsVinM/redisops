FROM rust:1.75 as builder

WORKDIR /app
COPY . .
RUN cd redisdb && cargo build --release

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/redisdb/target/release/redisops /usr/local/bin/

EXPOSE 6379

ENTRYPOINT ["redisops"]
