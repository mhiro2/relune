CREATE TABLE users (
    id BIGINT PRIMARY KEY
);

CREATE TABLE audit_log (
    id BIGINT PRIMARY KEY,
    message TEXT
);
