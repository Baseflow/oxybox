# Baseflow Oxybox: A Lean Multi-Tenant Monitoring Solution
---
[![Build docker container image](https://github.com/Baseflow/oxybox/actions/workflows/BUILD_AND_DEPLOY.yml/badge.svg)](https://github.com/Baseflow/oxybox/actions/workflows/BUILD_AND_DEPLOY.yml)
[![Docker Pulls](https://img.shields.io/docker/pulls/baseflow/oxybox.svg?maxAge=604800)](https://hub.docker.com/r/baseflow/oxybox/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)

Baseflow Oxybox is a lightweight and simplified alternative to the [Prometheus
Blackbox Exporter](https://github.com/prometheus/blackbox_exporter), designed
specifically for multi-tenant environments. It streamlines endpoint monitoring
by directly integrating with Mimir, eliminating the overhead associated with
traditional Blackbox Exporter deployments.

Oxybox is compatible with the [Prometheus Blackbox Exporter Dashboard](https://grafana.com/grafana/dashboards/7587-prometheus-blackbox-exporter/)
and can be used as a drop in replacement for the Blackbox exporter and Prometheus
![img](./img/screenshot.png)

---

## ❓ Why Baseflow Oxybox? The Challenges of Multi-Tenant Monitoring

In multi-tenant setups, leveraging `organisationId` for monitoring introduces significant complexities when using Prometheus Blackbox Exporter. Traditional setups often require:

* 📦 **Multiple Prometheus Helm Chart Installations**
  Each tenant typically requires its own Prometheus instance, resulting in a proliferation of deployments within the cluster.

* 🧩 **Bulky Integration with Grafana**
  Blackbox Exporter's reliance on Prometheus as a middleman introduces complexity in the data pipeline between exporters and Grafana.

* 🔁 **Unnecessary Intermediary**
  If Grafana Mimir is already used as the centralized metrics backend, inserting Prometheus solely to invoke the Blackbox Exporter becomes redundant. Prometheus ends up being just a relay.

This layered architecture increases operational overhead and can negatively impact performance and cluster stability.

---

## 🚀 Our Solution: Direct, Multi-Tenant Monitoring with Oxybox

**Baseflow Oxybox** solves these issues with a focused, streamlined design:

* 🧱 **Single Container Efficiency**
  A single Oxybox instance can monitor all endpoints across tenants with minimal resource usage.

* 🎯 **Direct Mimir Integration**
  Metrics are sent directly to Mimir with multi-tenancy enforced via `organisationId` tagging—no Prometheus required.

* ⚙️ **Reduced Operational Overhead**
  By eliminating Prometheus from the monitoring path, Oxybox reduces system complexity, improving reliability and maintainability.

> Oxybox is built to deliver a simpler, more resilient, and tenant-aware health check system for modern, multi-tenant infrastructure.

### 🐳 Running Oxybox via Docker

You can run Oxybox directly using the official Docker image published on Docker Hub:

```sh
docker run --rm \
  -v $(pwd)/example-config.yml:/app/config.yml \
  -e CONFIG_FILE=config.yml \
  -e DNS_HOSTS=8.8.8.8,1.1.1.1 \
  -e MIMIR_ENDPOINT=http://localhost:9009 \
  baseflow/oxybox:latest
```

### 📦 Notes:

* `-v $(pwd)/example-config.yml:/app/config.yml` mounts your local probe configuration into the container.
* Environment variables like `CONFIG_FILE`, `DNS_HOSTS`, and `MIMIR_ENDPOINT` are used to control runtime behavior.
* Replace `latest` with a specific version tag if you want to pin to a stable release.
* The working directory inside the container is `/app`.

You can find the image and tags here:
👉 [baseflow/oxybox on Docker Hub](https://hub.docker.com/r/baseflow/oxybox)

---

## 🔧 Configuration Overview

Oxybox supports two layers of configuration:

1. **Application Configuration** – Controlled via environment variables.
2. **Probe Configuration** – Defined in a YAML file to specify target endpoints per organization.

---

### 📝 Probe Configuration (YAML)

The probe configuration file defines how Oxybox should monitor endpoints per organization. Below is an example configuration:

```yaml
demo:
  organisation_id: demo
  polling_interval_seconds: 10
  targets:
    - url: https://www.google.com
    - url: https://www.github.com
      accepted_status_codes: [200, 301]
    - url: https://grafana.com/

organisationX:
  organisation_id: another-org
  polling_interval_seconds: 20
  targets:
    - url: http://www.example.com
```

Each top-level key (e.g., `demo`, `organisationX`) represents a distinct probe group. The configuration allows you to define:

* `organisation_id`: Logical identifier for the organization.
* `polling_interval_seconds`: Interval between health checks (in seconds).
* `targets`: List of endpoints to monitor.
  * `url`: The target URL.
  * `accepted_status_codes` (optional): A list of HTTP status codes considered successful.

---

### 🌍 Application Configuration (Environment Variables)

The following environment variables can be used to configure Oxybox’s runtime behavior:

| Name             | Example Value                                  | Default Value           |
| ---------------- | ---------------------------------------------- | ----------------------- |
| `CONFIG_FILE`    | `example-config.yml`                           | `config.yml`            |
| `DNS_HOSTS`      | `8.8.8.8, 1.1.1.1`                             | `1.1.1.1, 8.8.8.8`      |
| `MIMIR_ENDPOINT` | `http://mimir.grafana.svc.cluster.local:9090/` | `http://localhost:9009` |

These can be defined in a `.env` file or passed directly through your environment.
