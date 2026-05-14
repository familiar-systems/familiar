# syntax=docker/dockerfile:1.7
FROM rust:1-slim-bookworm AS builder
WORKDIR /build
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev ca-certificates && rm -rf /var/lib/apt/lists/*
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --release -p familiar-systems-campaign && \
    cp target/release/familiar-systems-campaign /build/campaign-bin

FROM gcr.io/distroless/cc-debian12
COPY --from=builder /build/campaign-bin /usr/local/bin/campaign
EXPOSE 3000
ENV PORT=3000
USER nonroot
ENTRYPOINT ["/usr/local/bin/campaign"]
