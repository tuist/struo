# Struo Cluster Onboarding

This runbook covers one-time production cluster setup for Struo. Day-to-day web
deploys build the image, apply the Struo website Helm release, point DNS at the
ingress, and run smoke tests.

## DNS

The deploy workflow keeps the `struo.run` A record in Cloudflare pointed at the
ingress-nginx LoadBalancer address after every release. It reads the Cloudflare
token from the `cloudflare-struo-token` secret, which the chart syncs from
`op://struo-k8s-production/cloudflare-struo-dns/credential` for the certificate
issuer. The token needs `Zone:Read` and `DNS:Edit` permissions for the
`struo.run` zone. No DNS controller has to run on the cluster.

Validate:

```bash
dig @1.1.1.1 struo.run A +short
```
