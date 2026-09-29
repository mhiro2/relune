CREATE TABLE items (
    id BIGINT PRIMARY KEY,
    qty INTEGER CHECK (qty >= 0),
    price NUMERIC(10, 2),
    CONSTRAINT price_positive CHECK (price > 0)
);
