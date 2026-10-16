# Resolve apt packages once, then preserve the built image for a pilot.
FROM rust:1.98.1-bookworm@sha256:93ce27a88655056a51dbdd8f5f2d7ddc071c7b0070fb288a37b5a285fc83971e

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
       bubblewrap python3 git make nodejs npm ca-certificates procps \
    && rm -rf /var/lib/apt/lists/*
RUN npm install -g @anthropic-ai/claude-code@2.1.287
RUN useradd --create-home --uid 1000 --shell /bin/bash pilot \
    && mkdir -p /workspace && chown pilot:pilot /workspace

ENV LANG=C.UTF-8 TERM=xterm DISABLE_AUTOUPDATER=1
USER pilot
WORKDIR /workspace
CMD ["sleep", "infinity"]
