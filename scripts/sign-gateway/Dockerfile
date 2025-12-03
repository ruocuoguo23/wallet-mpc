# syntax=docker/dockerfile:1

FROM alpine:3.20
RUN apk add --no-cache ca-certificates dumb-init
WORKDIR /app
COPY config/sign-gateway.yaml /app/config/sign-gateway.yaml
COPY target/sign-gateway/sign-gateway /usr/local/bin/sign-gateway
RUN chmod +x /usr/local/bin/sign-gateway
ENV PORT=8080 \
    HOST=0.0.0.0 \
    CONFIG_PATH=/app/config/sign-gateway.yaml
ENTRYPOINT ["/usr/bin/dumb-init", "--"]
CMD ["/usr/local/bin/sign-gateway", "/app/config/sign-gateway.yaml"]

