FROM ubuntu:latest

WORKDIR /app

ARG TARGET_TRIPLE

#бинарник c артефактов cборки

COPY --chmod=755 target/${TARGET_TRIPLE}/release/xiu /app/xiu

COPY --chmod=755 protocol/webrtc/src/clients/index.html /app/index.html
COPY --chmod=755 protocol/webrtc/src/clients/whep.js /app/whep.js

COPY --chmod=755 library/config/examples/config.toml /app/config.toml

COPY --chmod=755 docker/start.sh /app/start.sh