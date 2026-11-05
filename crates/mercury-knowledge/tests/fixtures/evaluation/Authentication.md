# Gateway authentication
Gateway validates tokens using introspection. ADR-21 chose introspection because immediate revocation is required. Alpha depends on Gateway. Gateway degradation blocks the Alpha rollout. Gateway has a runbook RB-8 that checks introspection latency before retrying.
