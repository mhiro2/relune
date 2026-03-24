CREATE TABLE public.users (
  id BIGINT PRIMARY KEY,
  name TEXT NOT NULL,
  email TEXT
);

CREATE TABLE public.posts (
  id BIGINT PRIMARY KEY,
  user_id BIGINT NOT NULL REFERENCES public.users(id),
  title TEXT NOT NULL,
  body TEXT,
  CONSTRAINT fk_posts_user FOREIGN KEY (user_id) REFERENCES public.users(id)
);
