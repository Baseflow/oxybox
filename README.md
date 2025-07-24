# Baseflow Oxybox: A Lean Multi-Tenant Monitoring Solution

Baseflow Oxybox is a lightweight and simplified alternative to the [Prometheus
Blackbox Exporter](https://github.com/prometheus/blackbox_exporter), designed
specifically for multi-tenant environments. It streamlines endpoint monitoring
by directly integrating with Mimir, eliminating the overhead associated with
traditional Blackbox Exporter deployments.

## Why Baseflow Oxybox? The Challenges of Multi-Tenant Monitoring

In multi-tenant setups, leveraging `organisationId` for monitoring presents
significant complexities with the Prometheus Blackbox Exporter. The conventional
approach often necessitates:

* **Multiple Prometheus Helm Chart Installations:** Each tenant typically
  requires its own Prometheus instance, leading to a proliferation of
  deployments within the cluster.
* **Bulky Integration with Grafana:** The Blackbox Exporter's reliance on
  Prometheus as an intermediary creates an overly complex data flow for
  visualization in Grafana.
* **Unnecessary Intermediary:** When Mimir is already in place as the central
  metrics store, routing monitoring data through Prometheus adds an redundant
  layer. Prometheus's role in this scenario is limited to calling Blackbox
  Exporter and then forwarding the results to Mimir.

This multi-layered architecture often results in an excessive number of
processes, contributing to increased operational burden and potential stability
issues within the cluster.

## Our Solution: Direct, Multi-Tenant Monitoring with Oxybox

Baseflow Oxybox addresses these challenges by offering a streamlined approach:

* **Single Container Efficiency:** A single Oxybox container can monitor all
  configured endpoints across multiple tenants.
* **Direct Mimir Integration:** Oxybox directly sends monitoring metrics to
  Mimir, complete with multi-tenancy enabled through `organisationId` tagging.
* **Reduced Operational Overhead:** By removing Prometheus as an intermediary,
  Oxybox significantly simplifies the monitoring stack, leading to fewer
  deployed components and a more robust, efficient system.

Oxybox aims to provide a leaner, more direct, and ultimately more reliable
solution for endpoint health checks in complex, multi-tenant infrastructures.

