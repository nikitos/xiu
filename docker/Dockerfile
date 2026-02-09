FROM ubuntu:latest

WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends libopus0 ca-certificates curl

ARG TARGETPLATFORM
ARG VERSION

# Скачиваем правильный бинарник под платформу из GitHub Releases
RUN set -eux; \
    case "$TARGETPLATFORM" in \
      linux/amd64)  TRIPLE="x86_64-unknown-linux-gnu" ;; \
      linux/arm64*) TRIPLE="aarch64-unknown-linux-gnu" ;; \
      *) echo "Unsupported TARGETPLATFORM=$TARGETPLATFORM" >&2; exit 1 ;; \
    esac; \
    ASSET="xiu-${VERSION}-${TRIPLE}"; \
    curl -fL "https://github.com/nikitos/xiu/releases/download/v${VERSION}/${ASSET}" -o /app/xiu; \
    chmod 755 /app/xiu