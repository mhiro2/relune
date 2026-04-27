CREATE TABLE orders (
    id BIGINT PRIMARY KEY
);

CREATE TABLE customers (
    id BIGINT PRIMARY KEY,
    tenant_id BIGINT NOT NULL
);
