CREATE TABLE users (id BIGINT PRIMARY KEY);
CREATE TABLE orders (id BIGINT PRIMARY KEY, user_id BIGINT);
CREATE INDEX orders_user_id_idx ON orders (user_id);
