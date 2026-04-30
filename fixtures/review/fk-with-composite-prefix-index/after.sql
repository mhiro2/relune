CREATE TABLE tenants (
    id BIGINT PRIMARY KEY
);

CREATE TABLE orders (
    id BIGINT PRIMARY KEY,
    tenant_id BIGINT NOT NULL,
    created_at TIMESTAMP NOT NULL,
    CONSTRAINT orders_tenant_id_fkey
        FOREIGN KEY (tenant_id)
        REFERENCES tenants (id)
);

CREATE INDEX orders_tenant_created_idx ON orders (tenant_id, created_at);
