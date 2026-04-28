CREATE TABLE users (
    id BIGINT PRIMARY KEY,
    legacy_email TEXT
);

CREATE TABLE orders (
    id BIGINT PRIMARY KEY,
    user_legacy_email TEXT,
    CONSTRAINT orders_user_legacy_email_fkey
        FOREIGN KEY (user_legacy_email)
        REFERENCES users (legacy_email)
);
