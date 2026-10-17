FROM rust:1.96 AS builder
WORKDIR /opt
COPY . .
RUN cargo build --release
RUN cp /opt/target/release/axum-web .
RUN cargo clean

FROM ubuntu:24.04
WORKDIR /opt
COPY --from=builder /opt/axum-web .
EXPOSE 8080
CMD ["./axum-web"]
