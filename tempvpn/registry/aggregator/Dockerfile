FROM rust:1.88-bookworm AS builder
WORKDIR /src
COPY . .
RUN cargo build --locked --release -p tempvpn-registry-aggregator

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=builder /src/target/release/tempvpn-registry-aggregator /usr/local/bin/tempvpn-registry-aggregator
ENV PORT=8080
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/tempvpn-registry-aggregator"]
