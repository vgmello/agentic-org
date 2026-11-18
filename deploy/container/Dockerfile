# rough first cut (milestone)
ARG BASE=debian:stable-slim@sha256:fbfb4059d2127120aa05b6e6064523d2102552f0cf0026ddf0701c12f1f8f3e0
FROM ${BASE}
ARG BIN=target/debug
RUN apt-get update \
 && apt-get install -y --no-install-recommends tini \
 && rm -rf /var/lib/apt/lists/*
RUN groupadd --gid 61000 agentic \
 && useradd --uid 61000 --gid 61000 --home-dir /data --no-create-home --shell /usr/sbin/nologin agentic \
 && install -d -o 61000 -g 61000 -m 0700 /data /secrets \
 && install -d -o root -g root -m 0755 /opt/agentic-org /opt/agentic-org/bin /etc/agentic-org
COPY --chown=root:root --chmod=0755 ${BIN}/agentic-org ${BIN}/agentic-org-office ${BIN}/agentic-org-web ${BIN}/agent-os /opt/agentic-org/bin/
COPY --chown=root:root --chmod=0644 deploy/container/setup.json /etc/agentic-org/setup.json
ENV PATH=/opt/agentic-org/bin:/usr/local/bin:/usr/bin:/bin \
    USER=agentic LOGNAME=agentic \
    ORG_OFFICE_DEV_KEY_DIR=/secrets/dev-keys
VOLUME ["/data", "/secrets"]
USER 61000:61000
WORKDIR /
EXPOSE 8443
ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["agentic-org", "serve", "--setup", "/etc/agentic-org/setup.json"]
