# Alpha and Beta
Alpha depends on Beta. Beta is a durable queue. ADR-17 chose Beta because message replay is required. Incident INC-12 affected Alpha and was resolved by enabling idempotent consumers. Alpha depends on Gateway for authentication.
