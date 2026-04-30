CREATE TABLE users (
    id BIGINT PRIMARY KEY
);

CREATE TABLE comments (
    id BIGINT PRIMARY KEY,
    author_id BIGINT,
    CONSTRAINT comments_author_id_fkey
        FOREIGN KEY (author_id)
        REFERENCES users (id)
        ON DELETE SET NULL
);
