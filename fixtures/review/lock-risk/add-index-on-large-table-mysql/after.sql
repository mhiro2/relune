CREATE TABLE orders (
    id BIGINT PRIMARY KEY,
    user_id BIGINT NOT NULL
);

CREATE INDEX orders_user_id_idx ON orders (user_id);
