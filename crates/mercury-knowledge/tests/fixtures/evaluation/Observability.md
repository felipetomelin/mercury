# Alpha observability
Alpha uses Datadog APM. Incident INC-9 affected Alpha: traces were missing because DD_SERVICE was incorrect. INC-9 was resolved by setting DD_SERVICE to alpha-api. The rollout dashboard checks error rate and p95 latency. Alpha publishes queue lag for Beta.
