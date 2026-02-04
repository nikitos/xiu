FROM ubuntu:latest

WORKDIR /app

ARG TARGET_TRIPLE

#бинарники c артефактов cборки
COPY --chmod=755 target/x86_64-unknown-linux-gnu/release/xiu /tmp/xiu_amd64
COPY --chmod=755 target/aarch64-unknown-linux-gnu/release/xiu /tmp/xiu_arm64

# Выбираем нужный для конкретной платформы
RUN set -eux; \
    case "$TARGETARCH" in \
      amd64) mv /tmp/xiu_amd64 /app/xiu ;; \
      arm64) mv /tmp/xiu_arm64 /app/xiu ;; \
      *) echo "Unsupported TARGETARCH=$TARGETARCH" >&2; exit 1 ;; \
    esac; \
    chmod 755 /app/xiu; \
    rm -f /tmp/xiu_*


COPY --chmod=755 protocol/webrtc/src/clients/index.html /app/index.html
COPY --chmod=755 protocol/webrtc/src/clients/whep.js /app/whep.js

COPY --chmod=755 library/config/examples/config.toml /app/config.toml

COPY --chmod=755 docker/start.sh /app/start.sh