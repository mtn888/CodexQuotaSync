FROM golang:1.24-alpine AS build

WORKDIR /src
COPY go.mod ./
COPY *.go ./
RUN CGO_ENABLED=0 GOOS=linux go build -trimpath -ldflags="-s -w" -o /out/codex-quota-sync-server .

FROM alpine:3.22

RUN apk add --no-cache tzdata \
    && mkdir -p /data \
    && chown 99:100 /data
COPY --from=build /out/codex-quota-sync-server /usr/local/bin/codex-quota-sync-server

USER 99:100
EXPOSE 8787
VOLUME ["/data"]
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD wget -q -O /dev/null "http://127.0.0.1:${PORT:-8787}/healthz" || exit 1
ENTRYPOINT ["/usr/local/bin/codex-quota-sync-server"]
