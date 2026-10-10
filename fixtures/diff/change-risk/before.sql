CREATE TABLE customers (
    id BIGINT PRIMARY KEY,
    name VARCHAR(120) NOT NULL,
    note VARCHAR(500),
    legacy_code TEXT
);

CREATE TABLE orders (
    id BIGINT PRIMARY KEY,
    customer_id BIGINT NOT NULL REFERENCES customers(id),
    total NUMERIC(10, 2) NOT NULL
);

CREATE INDEX orders_customer_id_idx ON orders (customer_id);

CREATE TABLE audit_log (
    id BIGINT PRIMARY KEY,
    message TEXT NOT NULL
);
