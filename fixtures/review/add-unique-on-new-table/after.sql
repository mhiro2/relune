CREATE TABLE orders (
    id BIGINT PRIMARY KEY
);

CREATE TABLE invitations (
    id BIGINT PRIMARY KEY,
    code VARCHAR(64)
);

CREATE UNIQUE INDEX invitations_code_key ON invitations (code);
