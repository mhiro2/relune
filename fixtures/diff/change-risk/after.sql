-- Change kind and risk vary independently in this diff:
--   customers.name    modified, safe (widened)
--   customers.note    modified, breaking (narrowed)
--   customers.status  added, risky (NOT NULL without a default)
--   customers.legacy_code removed, breaking (data lost)
--   orders.total      modified, safe (now nullable)
--   orders.shipped_at added, safe
--   coupons           added table, safe
--   audit_log         removed table, breaking
CREATE TABLE customers (
    id BIGINT PRIMARY KEY,
    name VARCHAR(500) NOT NULL,
    note VARCHAR(120),
    status TEXT NOT NULL
);

CREATE TABLE orders (
    id BIGINT PRIMARY KEY,
    customer_id BIGINT NOT NULL REFERENCES customers(id),
    total NUMERIC(10, 2),
    shipped_at TIMESTAMP
);

CREATE INDEX orders_customer_id_idx ON orders (customer_id);

CREATE TABLE coupons (
    id BIGINT PRIMARY KEY,
    code VARCHAR(32) NOT NULL
);
