CREATE TYPE order_status AS ENUM ('pending', 'paid', 'shipped');

CREATE TABLE orders (
    id BIGINT PRIMARY KEY,
    status order_status NOT NULL
);
