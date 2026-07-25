# mica + MinIO (Compose)

A local stack that shows **how session artifacts are stored** — recordings
(`/{id}.mp4`) and logs (`/{id}.log`) are uploaded to S3-compatible object
storage (MinIO here) when a session ends.

```bash
docker compose -f deploy/compose/docker-compose.yml up --build
```

- mica dashboard: <http://localhost:4444/admin>
- MinIO console: <http://localhost:9001> (`minioadmin` / `minioadmin`)

## Where files are stored — two knobs

**1. Local spool (always).** mica writes artifacts to disk first:

| flag | default | what |
|------|---------|------|
| `--video-output-dir` | `video` | recordings staged here |
| `--log-output-dir` | `logs` | logs staged here |

These are also what the dashboard's **Recordings** page reads from
(`GET /admin/api/recordings`) and what `/video/{id}.mp4` serves.

**2. S3 upload (optional).** When `--s3-bucket` is set, mica uploads each
finalized artifact on session stop and keeps the local copy as a spool.

| var / flag | purpose |
|------------|---------|
| `MICA_S3_BUCKET` / `--s3-bucket` | target bucket (empty = upload off) |
| `MICA_S3_REGION` / `--s3-region` | region (MinIO: any, e.g. `us-east-1`) |
| `MICA_S3_PREFIX` / `--s3-prefix` | key prefix or template (`$browserName/$fileName`) |
| `MICA_S3_FORCE_PATH_STYLE` / `--s3-force-path-style` | **`true` for MinIO/Ceph** — addresses `endpoint/bucket/key` |
| `AWS_ENDPOINT_URL_S3` | custom endpoint (read by the AWS SDK) |
| `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` | credentials |

## Verify the upload

```bash
# create + stop a session, then list the bucket:
docker compose -f deploy/compose/docker-compose.yml exec -T mica true   # (mica is running)
docker run --rm --network compose_default --entrypoint sh minio/mc -c \
  "mc alias set l http://minio:9000 minioadmin minioadmin && mc ls --recursive l/mica-artifacts"
```

You'll see `{session_id}.mp4` / `{session_id}.log` objects appear as sessions end.

## Notes

- **Recording needs a display-backed, self-recording browser image.** mica
  sets `ENABLE_VIDEO=true` on the browser container and expects the image to
  record its display to `/video`. Headless images (lightpanda,
  chrome-headless-shell) produce logs but no video. Use a
  selenoid/aerokube-style recording image for real `.mp4` output.
- **Backend networking.** mica's DockerBackend starts browser containers as
  siblings via the mounted `docker.sock`. Reaching their published ports from
  inside the mica container needs host networking on Linux
  (`network_mode: host`, drop the `ports:` mapping) — on Docker Desktop /
  OrbStack, run mica on the host for live sessions. The MinIO wiring shown
  here is independent of that and works as-is.
- **Revisiting S3 recordings in the UI.** The Recordings page lists the local
  spool. If you offload to S3 and prune local copies, listing/playback from
  S3 (presigned URLs) is a follow-up, not yet wired.
