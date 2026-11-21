CREATE TABLE orders (id BIGINT PRIMARY KEY, status VARCHAR(40) NOT NULL);
CREATE INDEX orders_status_idx ON orders (status);
