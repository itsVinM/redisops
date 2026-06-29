FROM rust:slim-bookworm

WORKDIR /app
COPY . .

RUN cargo build --release
RUN cargo test --test unit_test -- --nocapture

CMD ["cargo", "test", "--", "--nocapture"]
