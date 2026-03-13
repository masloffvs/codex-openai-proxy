#!/usr/bin/env bash

BASE_URL="http://localhost:18080"
API_KEY="test-key"
MODEL="gpt-5.3-codex"

### Health check
curl -s -X GET "${BASE_URL}/health" \
-H "Accept: application/json"

echo -e "\n---\n"

### Models list
curl -s -X GET "${BASE_URL}/v1/models" \
-H "Accept: application/json" \
-H "Authorization: Bearer ${API_KEY}"

echo -e "\n---\n"

### Chat completions (JSON)
curl -s -X POST "${BASE_URL}/v1/chat/completions" \
-H "Content-Type: application/json" \
-H "Accept: application/json" \
-H "Authorization: Bearer ${API_KEY}" \
-d '{
    "model": "'"${MODEL}"'",
    "stream": false,
    "messages": [
        {
            "role": "user",
            "content": "Reply with the single word OK"
        }
    ]
}'

echo -e "\n---\n"

### Chat completions (SSE)
curl -s -N -X POST "${BASE_URL}/v1/chat/completions" \
-H "Content-Type: application/json" \
-H "Accept: text/event-stream" \
-H "Authorization: Bearer ${API_KEY}" \
-d '{
    "model": "'"${MODEL}"'",
    "stream": true,
    "messages": [
        {
            "role": "user",
            "content": "Say hello in one short sentence"
        }
    ]
}'

echo -e "\n---\n"

### Compatibility route without /v1
curl -s -X POST "${BASE_URL}/chat/completions" \
-H "Content-Type: application/json" \
-H "Accept: application/json" \
-H "Authorization: Bearer ${API_KEY}" \
-d '{
    "model": "'"${MODEL}"'",
    "stream": false,
    "messages": [
        {
            "role": "user",
            "content": "Return any short confirmation"
        }
    ]
}'

echo ""
