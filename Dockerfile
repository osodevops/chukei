# Build a static-ish chukei binary and ship it distroless.
FROM rust:1-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release --bin chukei

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /src/target/release/chukei /usr/local/bin/chukei
COPY config/chukei-example.yaml /etc/chukei/chukei.yaml
EXPOSE 8443 9090
ENTRYPOINT ["/usr/local/bin/chukei"]
CMD ["up", "--config", "/etc/chukei/chukei.yaml"]
