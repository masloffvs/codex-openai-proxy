# Codex OpenAI Proxy

A reverse proxy that translates OpenAI Chat Completions API requests into ChatGPT Responses API calls, enabling tools like CLINE to work with ChatGPT Plus accounts.

## Architecture

The proxy sits between any OpenAI-compatible client and the ChatGPT backend:

```
Client (CLINE, etc.)
  |  OpenAI Chat Completions format
  v
codex-openai-proxy
  |  ChatGPT Responses API format
  v
ChatGPT Backend
```

All requests and responses are transparently converted between the two formats. Streaming is fully supported via SSE.

## Features

- OpenAI Chat Completions API-compatible interface
- Authentication via ChatGPT Plus `access_token` or standard OpenAI `api_key`
- Cloudflare bypass using browser-grade request headers
- Full streaming support (Server-Sent Events)
- Handles both string and array content formats in messages
- Tested with CLINE VS Code extension

## Installation

### Pre-built Binaries

Linux x86_64:

```bash
curl -fsSL https://github.com/masloffvs/codex-openai-proxy/releases/latest/download/codex-openai-proxy-x86_64-unknown-linux-gnu.tar.gz | tar -xz
chmod +x ./codex-openai-proxy
./codex-openai-proxy --port 8888 --auth-path ~/.codex/auth.json
```

macOS and Windows binaries are available on the [releases page](https://github.com/masloffvs/codex-openai-proxy/releases/latest). Release assets are published automatically for tags matching `v*`.

### Building from Source

```bash
git clone https://github.com/masloffvs/codex-openai-proxy.git
cd codex-openai-proxy
cargo build --release
./target/release/codex-openai-proxy --port 8888 --auth-path ~/.codex/auth.json
```

## Configuration

### CLI Reference

```
codex-openai-proxy [OPTIONS]

Options:
  -p, --port <PORT>          Port to listen on [default: 8080]
      --auth-path <PATH>     Path to Codex auth.json [default: ~/.codex/auth.json]
  -h, --help                 Print help
  -v, --version              Print version
```

### Authentication

The proxy reads credentials from the Codex `auth.json` file:

```json
{
  "access_token": "eyJ...",
  "account_id": "db1fc050-5df3-42c1-be65-9463d9d23f0b",
  "api_key": "sk-proj-..."
}
```

**Resolution order:** If both `access_token` and `account_id` are present, the proxy authenticates as a ChatGPT Plus account. Otherwise it falls back to `api_key` for standard OpenAI API access.

## API

### `GET /health`

Returns service status. Use this to verify the proxy is running.

### `POST /v1/chat/completions`

OpenAI-compatible chat completions endpoint. Also accessible at `/chat/completions`.

**Supported parameters:** `messages`, `model`, `temperature`, `max_tokens`, `stream`, `tools`.

**Request example:**

```json
{
  "model": "gpt-5",
  "messages": [{ "role": "user", "content": "Hello!" }]
}
```

This is internally converted to a Responses API payload:

```json
{
  "model": "gpt-5",
  "instructions": "You are a helpful AI assistant.",
  "input": [
    {
      "type": "message",
      "role": "user",
      "content": [{ "type": "input_text", "text": "Hello!" }]
    }
  ],
  "tools": [],
  "tool_choice": "auto",
  "store": false,
  "stream": false
}
```

The response from ChatGPT is then converted back to the standard Chat Completions format before being returned to the client.

## Usage with CLINE

CLINE (and most VS Code AI extensions) requires an HTTPS endpoint. Use an ngrok tunnel to expose the proxy:

```bash
# Create a static domain at https://dashboard.ngrok.com/domains first
ngrok http 8888 --domain=your-static-domain.ngrok-free.app
```

> **Security:** Use a unique ngrok domain and do not share it publicly. Anyone with access to the domain can make requests through your proxy.

Then configure CLINE in VS Code:

| Setting  | Value                                             |
| -------- | ------------------------------------------------- |
| Base URL | `https://your-static-domain.ngrok-free.app`       |
| Model    | `gpt-5` (or `gpt-4`)                              |
| API Key  | Any non-empty string (not validated by the proxy) |

### Verify the Connection

```bash
# Health check
curl https://your-static-domain.ngrok-free.app/health

# Test completion
curl -X POST https://your-static-domain.ngrok-free.app/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer test-key" \
  -d '{
    "model": "gpt-5",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

## Troubleshooting

### Connection refused

Ensure the proxy process is running and the port matches:

```bash
curl http://localhost:8080/health
```

### Authentication errors

Verify that `auth.json` exists and contains valid tokens:

```bash
cat ~/.codex/auth.json | jq .
```

### Debugging

Run the proxy with verbose logging to inspect request/response details:

```bash
RUST_LOG=debug cargo run -- --port 8080
```

Test with verbose curl output:

```bash
curl -v -X POST http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"model": "gpt-5", "messages": [{"role": "user", "content": "Test"}]}'
```

## Development

```bash
cargo build          # compile
cargo test           # run tests
cargo clippy         # lint
cargo fmt            # format
```

### Project Structure

- **`main.rs`** — HTTP server, route definitions, request handling
- **Format conversion** — functions translating between Chat Completions and Responses API schemas
- **`AuthData`** — authentication configuration and token resolution

The codebase is intentionally small and straightforward to extend. New endpoints are added as routes in `main.rs`; format translation logic is isolated in dedicated conversion functions.

## License

This project is part of the Codex ecosystem and follows the same licensing as the main Codex repository.
