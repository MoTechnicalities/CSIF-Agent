# CSIF-Agent OpenAI Compatibility Layer

## Overview

CSIF-Agent exposes an OpenAI-compatible Chat Completions surface so existing clients can point to a local endpoint with minimal or no code changes. The compatibility layer keeps the wire format familiar while the underlying behavior remains deterministic, auditable, and CPU-native.

## API Compatibility

| OpenAI Endpoint | CSIF-Agent Endpoint | Status |
| :--- | :--- | :--- |
| `POST /v1/chat/completions` | Same | Implemented |
| `GET /v1/models` | Same | Implemented |
| `GET /v1/models/:id` | Same | Implemented |
| `stream: true` | Same | Scaffolded |
| `functions` / `tools` | Same | Planned |

## Request Mapping

OpenAI-style requests are translated by extracting the latest `user` message and passing its content to the native CSIF-Agent query pipeline. System prompts are accepted for compatibility, but the agent does not depend on role-play instructions.

Example request:

```json
{
  "model": "csif-agent",
  "messages": [
    {"role": "system", "content": "You are a helpful assistant."},
    {"role": "user", "content": "What is a whale?"}
  ]
}
```

## Response Shape

The server returns the standard OpenAI chat-completion envelope:

```json
{
  "id": "chatcmpl-1716123456-1",
  "object": "chat.completion",
  "created": 1716123456,
  "model": "csif-agent",
  "choices": [
    {
      "index": 0,
      "message": {
        "role": "assistant",
        "content": "A whale is a mammal."
      },
      "finish_reason": "stop"
    }
  ]
}
```

## Authentication

The compatibility layer accepts bearer-style Authorization headers for client compatibility, but it does not require OpenAI API keys. Local deployments can ignore external credential management entirely.

## Client Examples

### Python OpenAI SDK

```python
import openai

openai.api_key = "any-value-works"
openai.base_url = "http://192.168.68.122:18080/v1"

response = openai.ChatCompletion.create(
    model="csif-agent",
    messages=[{"role": "user", "content": "What is a whale?"}]
)
print(response.choices[0].message.content)
```

### Curl

```bash
curl http://192.168.68.122:18080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer any-key" \
  -d '{
    "model": "csif-agent",
    "messages": [{"role": "user", "content": "What is a whale?"}]
  }'
```

### LangChain

```python
from langchain_openai import ChatOpenAI

llm = ChatOpenAI(
    base_url="http://192.168.68.122:18080/v1",
    api_key="any",
    model="csif-agent"
)
response = llm.invoke("What is a whale?")
```

## What the Client Sees

| Aspect | OpenAI Expectation | CSIF-Agent Reality |
| :--- | :--- | :--- |
| Endpoint | `POST /v1/chat/completions` | Same |
| Response shape | OpenAI-style JSON | Same |
| Latency | Network/cloud dependent | Local CPU execution |
| Determinism | Sampling varies | Same input, same output |
| Hallucination risk | Present | Explicitly constrained |
| Cost | Per-token billing | No required API spend |

## Limitations

- Streaming is not yet implemented.
- Streaming is currently a lightweight scaffold that emits a single SSE payload and a `[DONE]` terminator.
- Tool/function calling is not yet implemented.
- Model metadata is intentionally small, but now includes richer OpenAI-shaped fields and by-id lookup.
- System prompts are accepted but not used as instruction-tuning signals.

## Roadmap

- `v1.5.0`: Streaming support and richer `/v1/models` metadata
- `v1.5.0`: Streaming support and richer `/v1/models` metadata
- `v1.6.0`: Tool/function calling compatibility
- `v1.7.0`: Optional prompt-mode mapping for advanced compatibility

## Conclusion

CSIF-Agent can now be pointed at by OpenAI-compatible clients as a local, auditable, deterministic backend. Existing integrations can keep the same request/response shape while gaining CPU-native execution, zero cloud dependency, and explicit uncertainty handling.
