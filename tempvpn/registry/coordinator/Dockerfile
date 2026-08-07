FROM rust:1.86-bookworm AS builder
WORKDIR /app
COPY . .
RUN cargo build --locked --release -p tempvpn-session-coordinator

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --home /var/lib/tempvpn-coordinator coordinator
COPY --from=builder /app/target/release/tempvpn-session-coordinator /usr/local/bin/tempvpn-session-coordinator
USER coordinator
EXPOSE 8080 8443
ENTRYPOINT ["/usr/local/bin/tempvpn-session-coordinator"]
