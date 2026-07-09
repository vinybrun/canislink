FROM rust:1.86-bookworm as build
WORKDIR /src
COPY . .
RUN cargo build --release -p device-api -p signaling

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/device-api /usr/local/bin/device-api
COPY --from=build /src/target/release/signaling /usr/local/bin/signaling
