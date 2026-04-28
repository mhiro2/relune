CREATE TABLE users (
    id BIGINT PRIMARY KEY,
    email VARCHAR(255)
);

CREATE UNIQUE INDEX users_email_key ON users (email);
