# VM Metrics with Grafana Cloud

This project includes a separate VM metrics installer for Linux hosts. It deploys a small Docker Compose stack that runs Grafana Alloy on the server, scrapes host-level Linux metrics, and forwards them to a hosted Prometheus endpoint with `remote_write`.

This is intentionally separate from the main LalaSearch app stack so you can install, update, or remove observability without changing the application deployment.

## What it collects

- CPU usage and load
- Memory usage
- Disk usage
- Filesystem metrics
- Network traffic
- Basic host uptime and Linux host statistics

## Recommended hosted target

Grafana Cloud's free hosted Prometheus tier is a good fit for this setup. Create a Linux Server integration or hosted Prometheus endpoint there, then copy the remote write values into environment variables before running the installer.

Required values:

- `GRAFANA_CLOUD_PROMETHEUS_URL`
- `GRAFANA_CLOUD_PROMETHEUS_USERNAME`
- `GRAFANA_CLOUD_PROMETHEUS_PASSWORD`

## Install

Export the same SSH deployment variables you already use for app deployment:

```bash
export DEPLOY_HOST=203.0.113.10
export DEPLOY_USER=root
export DEPLOY_SSH_KEY="$(cat ~/.ssh/deploy_key)"
```

Export the Grafana Cloud credentials:

```bash
export GRAFANA_CLOUD_PROMETHEUS_URL=https://prometheus-prod-XX-prod-YY.grafana.net/api/prom/push
export GRAFANA_CLOUD_PROMETHEUS_USERNAME=1234567
export GRAFANA_CLOUD_PROMETHEUS_PASSWORD=glc_XXXXXXXXXXXXXXXX
```

Optional labels and install location:

```bash
export VM_METRICS_INSTANCE=lalasearch-prod
export VM_METRICS_ENVIRONMENT=prod
export METRICS_DEPLOY_DIR=/opt/lalasearch-metrics
export ALLOY_IMAGE_TAG=latest
```

The installer adds these labels to each sample:

- `vm_instance=<VM_METRICS_INSTANCE>`
- `environment=<VM_METRICS_ENVIRONMENT>`
- `service=lalasearch-vm`

Run the installer:

```bash
./scripts/install-vm-metrics.sh
```

## Files deployed to the server

The installer uploads these files into `METRICS_DEPLOY_DIR`:

- `docker-compose.metrics.yml`
- `config.alloy`
- `.env.metrics`

## Updating

Re-run the installer after changing the local config or environment variables:

```bash
./scripts/install-vm-metrics.sh
```

## Manual remote commands

```bash
cd /opt/lalasearch-metrics
docker compose --env-file .env.metrics -f docker-compose.metrics.yml ps
docker compose --env-file .env.metrics -f docker-compose.metrics.yml logs -f alloy
docker compose --env-file .env.metrics -f docker-compose.metrics.yml pull
docker compose --env-file .env.metrics -f docker-compose.metrics.yml up -d
```

## Removing the metrics stack

```bash
cd /opt/lalasearch-metrics
docker compose --env-file .env.metrics -f docker-compose.metrics.yml down
```
