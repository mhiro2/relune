-- Dogfood schema used to verify the relune review GitHub Action end-to-end.
-- This file is the canonical "application schema" for the dogfood-review
-- workflow at .github/workflows/dogfood-review.yaml.
--
-- The workflow runs `relune review` on every pull request that touches this
-- file, comparing the base ref against the PR head, and posts the report as a
-- sticky PR comment.

CREATE TABLE users (
    id BIGINT NOT NULL,
    email TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE posts (
    id BIGINT PRIMARY KEY,
    author_id BIGINT NOT NULL,
    title TEXT NOT NULL,
    body TEXT,
    published_at TIMESTAMPTZ,
    CONSTRAINT posts_author_id_fkey
        FOREIGN KEY (author_id)
        REFERENCES users (id)
);

CREATE INDEX posts_author_id_idx ON posts (author_id);

CREATE TABLE comments (
    id BIGINT PRIMARY KEY,
    post_id BIGINT NOT NULL,
    author_id BIGINT NOT NULL,
    body TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT comments_post_id_fkey
        FOREIGN KEY (post_id)
        REFERENCES posts (id),
    CONSTRAINT comments_author_id_fkey
        FOREIGN KEY (author_id)
        REFERENCES users (id)
);

CREATE INDEX comments_post_id_idx ON comments (post_id);
CREATE INDEX comments_author_id_idx ON comments (author_id);
