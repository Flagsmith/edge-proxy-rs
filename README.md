[![Feature Flag, Remote Config and A/B Testing platform, Flagsmith](https://raw.githubusercontent.com/Flagsmith/flagsmith/main/static-files/hero.png)](https://www.flagsmith.com/)

[![Join the Discord chat](https://img.shields.io/discord/517647859495993347)](https://discord.gg/hFhxNtXzgm)

[Flagsmith](https://flagsmith.com/) is an open source, fully featured, Feature Flag and Remote Config service. Use our
hosted API, deploy to your own private cloud, or run on-premise.

# Edge Proxy (Rust)

A high-performance Rust implementation of the Flagsmith Edge Proxy, built with Axum. This implementation is **6-10x faster** than the Python version while maintaining full API compatibility.

The Flagsmith Edge Proxy allows you to run an instance of the Flagsmith Engine close to your servers. If you are running
Flagsmith within a server-side environment and you want to have very low latency flags, you have two options:

1. Run the Edge Proxy and connect to it from your server-side SDKs
2. Run your server-side SDKs in [Local Evaluation Mode](https://docs.flagsmith.com/clients/overview#2---local-evaluation).

The main benefit to running the Edge Proxy is that you reduce your polling requests against the Flagsmith API itself.

## Performance

Benchmarked with 50 features, 1 CPU, 512MB RAM:

| Endpoint | Python | Rust | Speedup |
|----------|--------|------|---------|
| `GET /api/v1/flags` | 1,569 req/s | 10,075 req/s | **6.4x** |
| `POST /api/v1/identities` | 1,316 req/s | 14,110 req/s | **10.7x** |
| `GET /api/v1/environment-document` | 5,091 req/s | 17,904 req/s | **3.5x** |

## Quick Start

### Using Docker

```bash
# Pull the image
docker pull flagsmith/edge-proxy-rs:latest

# Run with mounted config
docker run -v ./config.json:/app/config.json -p 8000:8000 flagsmith/edge-proxy-rs:latest
```

### From Source

```bash
# Build
cargo build --release

# Create config.json (see Configuration section)

# Run server
cargo run --release
```

## Local Development

### Prerequisites

- [Rust](https://rustup.rs/) 1.85+
- [Docker](https://docs.docker.com/engine/install/) (optional)

### Setup

```bash
# Build
cargo build

# Run tests
cargo test

# Run with debug logging
RUST_LOG=debug cargo run
```

### Build and Run Docker Image Locally

```bash
# Build image
docker build . -t edge-proxy-rs-local

# Run image
docker run --rm -p 8000:8000 edge-proxy-rs-local
```

## Configuration

Edge Proxy expects to load configuration from `./config.json`.

### Configuration Options

```json
{
  "environment_key_pairs": [
    {
      "server_side_key": "ser.YOUR_SERVER_KEY",
      "client_side_key": "YOUR_CLIENT_KEY"
    }
  ],
  "api_url": "https://edge.api.flagsmith.com/api/v1",
  "api_poll_frequency_seconds": 60,
  "api_poll_timeout_seconds": 5,
  "server": {
    "host": "0.0.0.0",
    "port": 8000
  },
  "health_check": {
    "environment_update_grace_period_seconds": 120
  }
}
```

### Custom Config Path

```bash
export CONFIG_PATH=/path/to/config.json
cargo run
```

Or with Docker:

```bash
docker run \
    -e CONFIG_PATH=/var/foo.json \
    -v ./config.json:/var/foo.json \
    flagsmith/edge-proxy-rs:latest
```

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `CONFIG_PATH` | Path to config file | `./config.json` |
| `RUST_LOG` | Log level (`error`, `warn`, `info`, `debug`, `trace`) | `info` |
| `TOKIO_WORKER_THREADS` | Number of async worker threads | CPU cores |

## API Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/v1/flags` | GET | Get all flags for environment |
| `/api/v1/flags?feature=name` | GET | Get specific flag |
| `/api/v1/identities/` | GET | Get flags for identity (query param) |
| `/api/v1/identities/` | POST | Get flags for identity (with traits) |
| `/api/v1/environment-document` | GET | Get full environment document (server key only) |
| `/health` | GET | Health check |
| `/proxy/health` | GET | Health check (alias) |
| `/proxy/health/readiness` | GET | Readiness probe |
| `/proxy/health/liveness` | GET | Liveness probe |

## Load Testing

```bash
# Test flags endpoint
wrk -t4 -c20 -d10s \
    -H 'X-Environment-Key: your_client_key' \
    http://localhost:8000/api/v1/flags

# Test identities endpoint
wrk -t4 -c20 -d10s \
    -H 'X-Environment-Key: your_client_key' \
    -s post.lua \
    http://localhost:8000/api/v1/identities/
```

## Architecture

```
src/
├── main.rs                 # Entry point
├── lib.rs                  # Library exports
├── state.rs                # AppState type
├── error.rs                # Error types
├── config/                 # Configuration
│   ├── settings.rs         # AppSettings, key pairs
│   └── logging.rs          # Logging setup
├── routes/                 # HTTP layer
│   ├── mod.rs              # Router setup
│   ├── flags.rs            # GET /api/v1/flags
│   ├── identities.rs       # GET/POST /api/v1/identities
│   ├── health.rs           # Health checks
│   └── environment_document.rs
├── services/               # Business logic
│   └── environment.rs      # Flag evaluation, polling
├── cache/                  # Caching layer
│   └── environment.rs      # Environment document cache
└── models/                 # Domain models
    ├── request.rs          # IdentityWithTraits
    └── response.rs         # APIFeatureState
```

### Key Dependencies

- **axum** - Web framework by Tokio team
- **tokio** - Async runtime
- **flagsmith-flag-engine** - Flag evaluation engine
- **reqwest** - HTTP client
- **serde** - Serialization
- **tower-http** - Middleware (CORS, compression, tracing)

## Documentation

See [Edge Proxy documentation](https://docs.flagsmith.com/advanced-use/edge-proxy).

