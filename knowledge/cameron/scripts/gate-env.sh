# env for non-root gate runs (CA bundle copy readable by ubuntu)
export HOME=/home/ubuntu
export PATH=/home/ubuntu/.cargo/bin:/usr/local/bin:/usr/bin:/bin
CA=/home/ubuntu/ca-bundle.crt
export SSL_CERT_FILE=$CA CURL_CA_BUNDLE=$CA CARGO_HTTP_CAINFO=$CA GIT_SSL_CAINFO=$CA REQUESTS_CA_BUNDLE=$CA NIX_SSL_CERT_FILE=$CA
export RUSTUP_HOME=/home/ubuntu/.rustup CARGO_HOME=/home/ubuntu/.cargo
export NPM_CONFIG_USERCONFIG=/home/ubuntu/.npmrc BUN_INSTALL=/home/ubuntu/.bun npm_config_cafile=$CA NODE_EXTRA_CA_CERTS=$CA
export PATH=/home/ubuntu/.bun/bin:/opt/node22/bin:$PATH
export PLAYWRIGHT_BROWSERS_PATH=/home/ubuntu/.cache/ms-playwright
export PATH=/home/ubuntu/.npm-global/bin:$PATH
