# s3-explorer

A fast, self-contained **private S3 bucket explorer** written in Rust (axum + aws-sdk-s3). Built specifically to pair with [Railway Storage Buckets](https://docs.railway.com/storage-buckets) but works with any S3-compatible provider (AWS S3, Cloudflare R2, Backblaze B2, MinIO, Tigris).

Browse, upload, edit, preview, copy, rename, and delete objects in a private bucket through a clean web UI — with **Basic Auth** gating every endpoint and **zero egress fees** when run on Railway Private Networking.

- **Container size:** ~25 MB (multi-stage distroless build)
- **Cold start:** <1s on Railway
- **Memory:** ~15 MB RSS at idle
- **Single binary, no runtime, no Node, no Python**

---

## Features

| Capability | Endpoint |
|---|---|
| Browse bucket (folders, files, breadcrumbs, pagination) | `GET /browse` |
| Search keys by substring (server-side, paginated) | `GET /browse?q=...` |
| Drag-and-drop / multipart upload (folders included) | `POST /upload` |
| Stream upload (no full buffering) | `PUT /api/upload/stream` |
| Server-side replace (in-place edit) | `POST /replace` |
| Presigned GET URL (time-limited direct downloads) | `GET /api/presign?key=...` |
| Presigned PUT URL (browser direct upload) | `GET /api/presign/{key}` |
| Proxy stream (server-side fetch, with Range support) | `GET /files/{key}` |
| Inline preview (image, text, PDF, audio, video) | `GET /preview/{key}` |
| Text editor with `PUT` to S3 | `GET /edit`, `PUT /api/objects/{key}` |
| Copy / rename / move with overwrite detection | `GET /copy`, `POST /api/copy`, `POST /api/move` |
| WebP thumbnail generation (cached in-memory, ETag-keyed) | `GET /api/thumb/{key}` |
| Recursive folder delete (paginated) | `POST /api/delete-prefix/{prefix}` |
| Single object delete | `DELETE /api/objects/{key}` |
| Liveness / readiness / version | `GET /health`, `/ready`, `/version` |

---

## Quick start (local)

```bash
git clone <this-repo> s3-explorer
cd s3-explorer
cp .env.example .env
# Edit .env and set BUCKET, ACCESS_KEY_ID, SECRET_ACCESS_KEY
cargo run --release
# → open http://127.0.0.1:3000
```

The release build takes ~3 minutes from cold. The first request to `/health` will fail with a 503 until your S3 credentials are correct; that's expected.

### Requirements

- Rust **1.83+** (uses edition 2021, axum 0.8, aws-sdk-s3 1.x)
- An S3-compatible bucket + access key

---

## Environment variables

| Var | Required | Default | Description |
|---|---|---|---|
| `BUCKET` | ✅ | — | Bucket name (e.g. `my-bucket-abc123` for Railway) |
| `ACCESS_KEY_ID` | ✅ | — | S3 access key |
| `SECRET_ACCESS_KEY` | ✅ | — | S3 secret key |
| `ENDPOINT` | — | `https://storage.railway.app` | S3-compatible endpoint |
| `REGION` | — | `auto` | S3 region (Railway uses `auto`) |
| `EXPLORER_USER` | ⚠️ recommended | — | Basic Auth username (blank = no auth) |
| `EXPLORER_PASS` | ⚠️ recommended | — | Basic Auth password (blank = no auth) |
| `PORT` | — | `3000` | HTTP listen port (Railway injects this) |
| `PRESIGN_TTL_SECS` | — | `900` | Presigned URL lifetime (seconds) |
| `MAX_UPLOAD_BYTES` | — | `104857600` | Max single-upload size (default 100 MiB) |
| `RUST_LOG` | — | `info,s3_explorer=info,tower_http=info` | Tracing filter |

> ⚠️ **Production security:** if `EXPLORER_USER` or `EXPLORER_PASS` is unset, the explorer is fully public. Only do this when the service is reachable only over Railway Private Networking, or you genuinely want anonymous access (e.g. a public asset browser).

---

## Deploying to Railway

This is the recommended deployment. Railway Storage Buckets live in the same project as your services, so you can wire credentials via **Variable References** — no secrets ever appear in your `.env` or git history.

### 1. Push the repo to GitHub

```bash
git init && git add . && git commit -m "init"
gh repo create s3-explorer --public --source=. --push
```

### 2. Create a new Railway project

Go to [railway.com/new](https://railway.com/new), select **Deploy from GitHub repo**, pick `s3-explorer`.

Railway will detect the `Dockerfile` and print:

```
==========================
Using detected Dockerfile!
==========================
```

Build takes ~3-4 minutes the first time (Rust cold cache). Subsequent deploys reuse the cache and finish in seconds.

### 3. Create a Storage Bucket

In the same project, click **+ New → Bucket → Storage Bucket**, pick a region (`sjc`, `iad`, `ams`, or `sin`) and a name.

> The bucket region is **immutable** after creation.

### 4. Wire credentials with Variable References

Open the explorer's **Variables** tab and add these variable references pointing to the bucket:

| Variable | Value |
|---|---|
| `BUCKET` | `${{BucketName.AWS_S3_BUCKET_NAME}}` |
| `ACCESS_KEY_ID` | `${{BucketName.AWS_ACCESS_KEY_ID}}` |
| `SECRET_ACCESS_KEY` | `${{BucketName.AWS_SECRET_ACCESS_KEY}}` |
| `ENDPOINT` | `${{BucketName.AWS_ENDPOINT_URL}}` |
| `REGION` | `${{BucketName.AWS_DEFAULT_REGION}}` |

> `BucketName` is whatever you named the bucket. The `${{...}}` syntax is Railway's [variable reference](https://docs.railway.com/variables/reference#template-syntax) — it resolves to the bucket's credential at deploy time.

The explorer's code reads `BUCKET`, `ACCESS_KEY_ID`, `SECRET_ACCESS_KEY`, `ENDPOINT`, `REGION` directly. The Railway-provided `AWS_*` names don't have to match.

### 5. (Recommended) Set Basic Auth

In the **Variables** tab:

| Variable | Value |
|---|---|
| `EXPLORER_USER` | a username of your choice |
| `EXPLORER_PASS` | a strong password |

All routes (including `/health`) will now require HTTP Basic Auth. Your browser will prompt on first visit.

### 6. Generate a public domain

In the explorer's **Settings** tab → **Networking** → **Generate Domain**. Railway gives you `<service-name>.up.railway.app` on HTTPS automatically.

> If you set a custom domain, point it at the service and Railway handles the TLS certificate.

### 7. Verify

Open the public URL. You should land in the bucket root. If you see the explorer chrome but `502 Bad Gateway`, your S3 credentials are wrong — re-check the variable references in step 4.

---

## Deploy via Railway CLI

If you prefer the CLI:

```bash
npm install -g @railway/cli
railway login
railway init   # link or create a project
railway up     # deploys from current directory
railway variables set BUCKET=my-bucket
railway variables set ACCESS_KEY_ID=...
railway variables set SECRET_ACCESS_KEY=...
railway variables set EXPLORER_USER=admin
railway variables set EXPLORER_PASS='use-a-secret-here'
railway open   # open the dashboard
```

---

## `railway.toml` reference

```toml
[build]
builder = "DOCKERFILE"
dockerfilePath = "Dockerfile"

[deploy]
startCommand = "/s3-explorer"
healthcheckPath = "/health"
healthcheckTimeout = 30
restartPolicyType = "ON_FAILURE"
restartPolicyMaxRetries = 3
```

| Field | Why |
|---|---|
| `builder = "DOCKERFILE"` | Force the multi-stage build. Without this, Railway's Railpack might pick up `Cargo.toml` and try Nixpacks (slower, no `release` profile, no `strip`). |
| `healthcheckPath = "/health"` | Must start with `/`. Required for zero-downtime deploys. |
| `healthcheckTimeout = 30` | Rust starts in <2s, so 30s is plenty. Bump to `300` if you ever see healthcheck flakiness. |
| `restartPolicyType = "ON_FAILURE"` | Restart on crash, don't restart on intentional shutdown. |
| `startCommand = "/s3-explorer"` | Distroless image has no shell, so the binary is invoked directly. |

The JSON schema for autocomplete is at `https://railway.com/railway.schema.json` — add it to `railway.json` if you switch formats.

---

## Dockerfile notes

Multi-stage build:

1. **`builder`** — `rust:1.83-bookworm` with `libssl-dev` and `pkg-config`. Pre-builds a stub `main.rs` against `Cargo.toml` first to cache dependencies in a separate layer; then copies the real source and re-builds. This is the standard Rust-on-Docker pattern and shaves minutes off subsequent deploys.
2. **`runtime`** — `gcr.io/distroless/cc-debian12`. No shell, no package manager, no CVEs to patch. The final image is **~25 MB**.

The container runs as `nonroot:nonroot` and exposes the port the `PORT` env var points to (Railway injects it; default `3000`).

---

## Using with non-Railway S3 providers

The explorer speaks standard S3 SigV4, so it works with anything S3-compatible. Just point `ENDPOINT` at the provider and supply that provider's keys:

| Provider | `ENDPOINT` |
|---|---|
| AWS S3 (us-east-1) | `https://s3.us-east-1.amazonaws.com` |
| Cloudflare R2 | `https://<accountid>.r2.cloudflarestorage.com` |
| Backblaze B2 | `https://s3.<region>.backblazeb2.com` |
| MinIO (self-hosted) | `https://minio.example.com` |
| Railway Bucket | `https://storage.railway.app` |
| Tigris | `https://fly.storage.tigris.dev` |

> The explorer uses `force_path_style(true)`, so it works with both **virtual-hosted-style** (default for new Railway buckets) and **path-style** URLs (required for some legacy buckets).

---

## Architecture

```
Browser ──Basic Auth──► /browse, /preview, /edit, /files/{key}, /api/*
   │
   │  templates/  ──►  askama renders server-side HTML
   │  static/     ──►  app.js, app.css, favicon.svg (inlined via include_bytes!)
   │
Server (axum on tokio)
   │
   ├── aws-sdk-s3  ──►  ListObjectsV2, GetObject, PutObject, DeleteObject,
   │                    CopyObject, HeadObject, CreateMultipartUpload, ...
   │                    + presigning via aws-sigv4
   │
   └── image::ImageOps  ──►  on-the-fly WebP thumbnails (in-memory cache)
```

Every request that touches the bucket uses the same S3 client (a single `aws_sdk_s3::Client` cloned across the app). Upload streams are collected into `bytes::Bytes` and bounded by `MAX_UPLOAD_BYTES`. Downloads and proxy responses stream via `tokio_util::io::ReaderStream` → `axum::body::Body::from_stream`.

### Health model

`/health` is a **liveness** check — it returns 200 if the process is up. It also performs a `head_bucket` and reports `status: degraded` with the error string if credentials are wrong (HTTP 503). This is useful for differentiating "process crashed" from "process running but can't reach S3".

`/ready` is a true readiness probe and only returns 200 when the bucket is reachable. `/version` is a static JSON response.

Railway only uses the `healthcheckPath` you give it (`/health` here) for zero-downtime rollout gating, not for ongoing monitoring.

---

## Security checklist

- [x] **Basic Auth** on all routes (`auth.rs` middleware). Sends `WWW-Authenticate` challenge on missing/bad creds.
- [x] **Path sanitization** — every S3 key is passed through `sanitize_key()` which blocks `..`, backslashes, and trailing-slash prefix tricks.
- [x] **Length caps** — uploads enforce `MAX_UPLOAD_BYTES` (default 100 MiB). Adjust with the env var.
- [x] **No `dangerouslySetInnerHTML`** in templates — Askama auto-escapes. Pre-rendered HTML rows from `render_files()` are escaped at the source.
- [x] **CORS** — `CorsLayer::permissive()` because the browser talks to the same origin. Lock down with `CorsLayer::new()` if you split the frontend.
- [x] **Security headers** — `x-content-type-options: nosniff` and `referrer-policy: no-referrer` set on every response.
- [x] **Non-root container** — runs as `nonroot:nonroot` in distroless.
- [x] **No secret logging** — `RUST_LOG=info` by default. Error responses never include credentials.
- [x] **No public egress** — works fully over Railway Private Networking; nothing leaves the project network.

### What to add before production

- **Rate limiting** on `/api/upload` and `/api/copy` (e.g. `tower-governor`).
- **Per-user audit log** if `EXPLORER_USER` becomes more than one account.
- **CORS lockdown** if you serve the explorer behind a different domain than the API.

---

## API reference

| Method | Path | Purpose |
|---|---|---|
| `GET`  | `/` | 307 → `/browse` |
| `GET`  | `/browse?prefix=...&cursor=...&q=...` | Folder listing (HTML) |
| `POST` | `/upload` | Multipart upload (multiple `files[]`, optional `prefix`) |
| `POST` | `/replace` | Form-based in-place replace (single file) |
| `GET`  | `/files/{key}` | Proxy stream (`?download=1` forces `Content-Disposition`) |
| `GET`  | `/preview/{key}` | Inline preview (HTML) |
| `GET`  | `/edit?key=...` | Text editor (HTML) |
| `GET`  | `/copy?from=...` | Copy form (HTML) |
| `GET`  | `/favicon.ico` | SVG favicon |
| `GET`  | `/static/{path}` | Inlined assets |
| `PUT`  | `/api/upload/stream` | Raw-body upload (`?key=...&type=...`) |
| `GET`  | `/api/presign?key=...` | Presigned GET URL (JSON) |
| `GET`  | `/api/presign/{key}` | Presigned PUT URL (JSON) |
| `GET`  | `/api/objects` | Alias for `/browse` |
| `GET`  | `/api/objects/{key}` | Raw object bytes |
| `PUT`  | `/api/objects/{key}` | Text edit save (JSON or form) |
| `DELETE` | `/api/objects/{key}` | Delete one object |
| `POST` | `/api/delete-prefix/{prefix}` | Recursive folder delete |
| `GET`  | `/api/thumb/{key}` | 256×256 WebP thumbnail |
| `POST` | `/api/copy` | Server-side copy (`{from, to, overwrite}`) |
| `POST` | `/api/move` | Server-side move (`{from, to, overwrite}`) |
| `GET`  | `/health` | Liveness + S3 reachability |
| `GET`  | `/ready` | Readiness (200 only if bucket is reachable) |
| `GET`  | `/version` | Static version JSON |

### Error shape

Every error response is JSON:

```json
{ "error": "message", "code": "S3", "status": 502 }
```

HTTP codes used: `400` (bad input), `401` (no auth), `404` (not found), `405` (wrong method), `413` (too large), `502` (S3 upstream error), `503` (degraded).

---

## Development

```bash
cargo run                    # debug build, with pretty logs
cargo run --release          # release build
cargo build --release        # build only
cargo check                  # type-check only (~10s)
```

Logging respects `RUST_LOG`. Useful presets:

```bash
RUST_LOG=debug cargo run               # noisy
RUST_LOG=info,s3_explorer=trace cargo run   # see every S3 call
RUST_LOG=warn cargo run                # quiet
```

The dev server listens on the port in `PORT` (default `3000`). Hot-reload is **not** wired up; this is a small enough codebase to iterate by `cargo run` after `Ctrl-C`.

### Project layout

```
src/
├── main.rs            # axum router, middleware, static serving
├── s3.rs              # S3Config + build_context() (client construction)
├── state.rs           # AppState, AuthConfig, public_base_url()
├── auth.rs            # Basic Auth middleware
├── error.rs           # AppError + IntoResponse
└── routes/
    ├── mod.rs         # IndexPage template, FileEntry, FolderEntry,
    │                  # render_folders, render_files, render_crumbs,
    │                  # human_size, guess_content_type, is_text_key
    ├── list.rs        # /browse + /api/objects
    ├── upload.rs      # /upload, /replace, /api/upload/stream
    ├── download.rs    # /files/{key}, /api/presign
    ├── preview.rs     # /preview/{key}, /api/objects/{key}
    ├── edit.rs        # /edit, PUT /api/objects/{key}
    ├── copy.rs        # /copy, /api/copy, /api/move
    ├── delete.rs      # DELETE endpoints
    ├── thumb.rs       # /api/thumb/{key}
    └── health.rs      # /health, /ready, /version

templates/             # askama templates (compiled in at build time)
├── index.html         # main browser
├── preview.html
├── edit.html
└── copy.html

static/                # inlined via include_bytes! in main.rs
├── app.js
├── app.css
└── favicon.svg
```

---

## Troubleshooting

**`502 Bad Gateway` on every request**
S3 credentials are wrong. Check `BUCKET`, `ACCESS_KEY_ID`, `SECRET_ACCESS_KEY` against the bucket's Credentials tab. The `/health` endpoint's JSON body contains the actual upstream error.

**`403 Forbidden` only on `PUT` / multipart**
The bucket credential is read-only, or the bucket has a policy restricting writes. Confirm with `aws s3 cp` or the bucket dashboard.

**Healthcheck fails at deploy time but app runs fine**
Bump `healthcheckTimeout` to `300` in `railway.toml`. Rust is fast but the S3 client warms up on first call.

**Upload works for small files but fails on large files**
Increase `MAX_UPLOAD_BYTES`. On Railway, the body is fully buffered in memory, so don't go above ~500 MB without switching to multipart streaming.

**`port already in use` after redeploy**
Railway sends `SIGTERM` and waits for graceful shutdown; the Rust server's `tokio` runtime drains in-flight requests. If you see this, check that nothing is doing synchronous work in a request handler.

**`Path segments must not start with *` on startup**
You're using an old `axum` 0.7-style route. The codebase uses axum 0.8 syntax (`{key}` not `*key`). Pull the latest version.

**`x-content-type-options: nosniff` blocks previews**
By design. If you set this in a reverse proxy too, the browser may refuse to render images inline. The explorer's own response already sets it.

---

## License

MIT
