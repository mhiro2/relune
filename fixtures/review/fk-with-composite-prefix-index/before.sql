CREATE TABLE orders (
    id BIGINT PRIMARY KEY,
    tenant_id BIGINT NOT NULL,
    created_at TIMESTAMP NOT NULL
);

CREATE INDEX orders_tenant_created_idx ON orders (tenant_id, created_at);
