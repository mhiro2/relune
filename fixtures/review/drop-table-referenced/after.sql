CREATE TABLE orders (
    id BIGINT PRIMARY KEY,
    user_id BIGINT,
    CONSTRAINT orders_user_id_fkey
        FOREIGN KEY (user_id)
        REFERENCES legacy_users (id)
);
