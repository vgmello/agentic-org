FROM ghcr.io/vectorize-io/hindsight-api@sha256:084af5a6522d67f5905f7992cdb3d910d778496e5dd2566caec7cf82ed2e7d8c

USER root
COPY requirements.txt /tmp/kit-memory-requirements.txt
RUN --mount=type=secret,id=kit_ca,mode=0444 \
    if [ -s /run/secrets/kit_ca ]; then export SSL_CERT_FILE=/run/secrets/kit_ca; fi; \
    uv pip install --system-certs --python /app/api/.venv/bin/python --no-cache --no-deps --only-binary :all: --require-hashes -r /tmp/kit-memory-requirements.txt \
    && uv pip check --python /app/api/.venv/bin/python
COPY runtime.json download_models.py /opt/agent-kit/
RUN mkdir -p /opt/agent-kit/models && chown -R hindsight:hindsight /opt/agent-kit
USER hindsight
RUN --mount=type=secret,id=kit_ca,mode=0444 \
    if [ -s /run/secrets/kit_ca ]; then export SSL_CERT_FILE=/run/secrets/kit_ca; fi; \
    /app/api/.venv/bin/python /opt/agent-kit/download_models.py

ENV HINDSIGHT_API_LLM_PROVIDER=none \
    HINDSIGHT_API_RETAIN_EXTRACTION_MODE=chunks \
    HINDSIGHT_API_ENABLE_OBSERVATIONS=false \
    HINDSIGHT_API_ENABLE_AUTO_CONSOLIDATION=false \
    HINDSIGHT_API_ENABLE_GRAPH_RETRIEVAL=false \
    HINDSIGHT_API_ENABLE_TEMPORAL_RETRIEVAL=false \
    HINDSIGHT_API_MCP_ENABLED=false \
    HINDSIGHT_API_SKIP_LLM_VERIFICATION=true \
    HINDSIGHT_API_EMBEDDINGS_PROVIDER=onnx \
    HINDSIGHT_API_EMBEDDINGS_ONNX_MODEL_PATH=/opt/agent-kit/models/embedding/onnx/model.onnx \
    HINDSIGHT_API_EMBEDDINGS_ONNX_TOKENIZER_NAME_OR_PATH=/opt/agent-kit/models/embedding \
    HINDSIGHT_API_EMBEDDINGS_ONNX_DEVICE=cpu \
    HINDSIGHT_API_RERANKER_PROVIDER=onnx \
    HINDSIGHT_API_RERANKER_ONNX_MODEL_PATH=/opt/agent-kit/models/reranker/onnx/model.onnx \
    HINDSIGHT_API_RERANKER_ONNX_TOKENIZER_NAME_OR_PATH=/opt/agent-kit/models/reranker \
    HINDSIGHT_API_RERANKER_ONNX_THREADS=2 \
    OMP_NUM_THREADS=2 \
    HF_HUB_OFFLINE=1 \
    TRANSFORMERS_OFFLINE=1 \
    HF_HUB_DISABLE_TELEMETRY=1 \
    DO_NOT_TRACK=1
