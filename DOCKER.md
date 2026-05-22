# Docker Guide

## Build Images

Build local test image:

```bash
docker build -t csif-agent:test .
```

Build multi-arch images (AMD64 + ARM64):

```bash
./docker-build.sh v1.1.0
```

Build and push to GHCR:

```bash
PUSH=true ./docker-build.sh v1.1.0
```

## Run Container

Quick run:

```bash
./docker-run.sh
```

Or with docker compose:

```bash
docker compose up -d
```

## Environment Variables

- `CSIF_BANK_PATH`: Path to the RWIF crystal bank file. Default in container is `/data/my_brain.rwif`.
- `CSIF_PORT`: HTTP listen port inside the container. Default is `8080`.
- `IMAGE`: Override image name for scripts.
- `HOST_PORT`: Override host port in `docker-run.sh`.
- `CONTAINER_NAME`: Override container name in `docker-run.sh`.
- `DATA_VOLUME`: Override Docker volume name in `docker-run.sh`.

## Port Configuration

By default, the agent listens on port `8080` inside the container. You can change both the internal and external ports:

| Scenario | Command |
| :--- | :--- |
| Change external port only | `docker run -p 3000:8080 csif-agent:latest` |
| Change both ports | `docker run -e CSIF_PORT=3000 -p 3000:3000 csif-agent:latest` |
| Run multiple agents | Use different external ports for each |

**Why this matters:** Run different agents for different domains (legal, finance, personal) on different ports, each with its own crystal bank volume.

## Volume Mounts

- Container volume mount point: `/data`
- RWIF persistence file: `/data/my_brain.rwif`
- Named volume default: `csif-agent-data`

This ensures memory survives container restarts and upgrades.

## Networking

- Container listens on `0.0.0.0:8080`
- Default host mapping is `8080:8080`
- Health endpoint: `GET /health`

## Troubleshooting

- Port already in use:

```bash
fuser -k 8080/tcp
```

- Check health:

```bash
curl http://localhost:8080/health
```

- View logs:

```bash
docker logs -f csif-agent
```

- Reset persisted knowledge:

```bash
docker volume rm csif-agent-data
```

## Security Considerations

- Container runs as non-root user (`uid=10001`)
- No privileged mode required
- Minimal Alpine runtime image
- No shell access required for normal operation
- Persisted data is isolated to `/data`
