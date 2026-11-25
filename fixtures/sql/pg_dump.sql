--
-- PostgreSQL database dump
--

\restrict fixture

-- Dumped from database version 18
-- Dumped by pg_dump version 18

SET statement_timeout = 0;
SET lock_timeout = 0;
SET idle_in_transaction_session_timeout = 0;
SET transaction_timeout = 0;
SET client_encoding = 'UTF8';
SET standard_conforming_strings = on;
SELECT pg_catalog.set_config('search_path', '', false);
SET check_function_bodies = false;
SET xmloption = content;
SET client_min_messages = warning;
SET row_security = off;

--
-- Name: email; Type: DOMAIN; Schema: public; Owner: postgres
--

CREATE DOMAIN public.email AS text
	CONSTRAINT email_check CHECK ((VALUE ~ '@'::text));


ALTER DOMAIN public.email OWNER TO postgres;

--
-- Name: mood; Type: TYPE; Schema: public; Owner: postgres
--

CREATE TYPE public.mood AS ENUM (
    'sad',
    'ok',
    'happy'
);


ALTER TYPE public.mood OWNER TO postgres;

--
-- Name: CAST (text AS public.email); Type: CAST; Schema: -; Owner: -
--

CREATE CAST (text AS public.email) WITH INOUT AS IMPLICIT;


--
-- Name: archive(date); Type: PROCEDURE; Schema: public; Owner: postgres
--

CREATE PROCEDURE public.archive(IN before date)
    LANGUAGE sql
    AS $$ DELETE FROM orders WHERE placed_at < before $$;


ALTER PROCEDURE public.archive(IN before date) OWNER TO postgres;

--
-- Name: on_ddl(); Type: FUNCTION; Schema: public; Owner: postgres
--

CREATE FUNCTION public.on_ddl() RETURNS event_trigger
    LANGUAGE plpgsql
    AS $$ BEGIN END $$;


ALTER FUNCTION public.on_ddl() OWNER TO postgres;

--
-- Name: total(integer); Type: AGGREGATE; Schema: public; Owner: postgres
--

CREATE AGGREGATE public.total(integer) (
    SFUNC = int4pl,
    STYPE = integer
);


ALTER AGGREGATE public.total(integer) OWNER TO postgres;

--
-- Name: ===; Type: OPERATOR; Schema: public; Owner: postgres
--

CREATE OPERATOR public.=== (
    FUNCTION = texteq,
    LEFTARG = text,
    RIGHTARG = text
);


ALTER OPERATOR public.=== (text, text) OWNER TO postgres;

SET default_tablespace = '';

--
-- Name: orders; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.orders (
    id bigint NOT NULL,
    user_id integer NOT NULL,
    placed_at date NOT NULL
)
PARTITION BY RANGE (placed_at);


ALTER TABLE public.orders OWNER TO postgres;

SET default_table_access_method = heap;

--
-- Name: order_counts; Type: MATERIALIZED VIEW; Schema: public; Owner: postgres
--

CREATE MATERIALIZED VIEW public.order_counts AS
 SELECT user_id,
    count(*) AS n
   FROM public.orders
  GROUP BY user_id
  WITH NO DATA;


ALTER MATERIALIZED VIEW public.order_counts OWNER TO postgres;

--
-- Name: orders_2026; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.orders_2026 (
    id bigint CONSTRAINT orders_id_not_null NOT NULL,
    user_id integer CONSTRAINT orders_user_id_not_null NOT NULL,
    placed_at date CONSTRAINT orders_placed_at_not_null NOT NULL
);


ALTER TABLE public.orders_2026 OWNER TO postgres;

--
-- Name: users; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.users (
    id integer NOT NULL,
    email public.email NOT NULL,
    mood public.mood,
    name text
);
ALTER TABLE ONLY public.users ALTER COLUMN email SET STATISTICS 500;
ALTER TABLE ONLY public.users ALTER COLUMN name SET COMPRESSION pglz;


ALTER TABLE public.users OWNER TO postgres;

--
-- Name: users_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

ALTER TABLE public.users ALTER COLUMN id ADD GENERATED ALWAYS AS IDENTITY (
    SEQUENCE NAME public.users_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1
);


--
-- Name: orders_2026; Type: TABLE ATTACH; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.orders ATTACH PARTITION public.orders_2026 FOR VALUES FROM ('2026-01-01') TO ('2027-01-01');


--
-- Name: orders orders_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.orders
    ADD CONSTRAINT orders_pkey PRIMARY KEY (id, placed_at);


--
-- Name: orders_2026 orders_2026_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.orders_2026
    ADD CONSTRAINT orders_2026_pkey PRIMARY KEY (id, placed_at);


--
-- Name: users users_email_key; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.users
    ADD CONSTRAINT users_email_key UNIQUE (email);


--
-- Name: users users_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.users
    ADD CONSTRAINT users_pkey PRIMARY KEY (id);

ALTER TABLE public.users CLUSTER ON users_pkey;


--
-- Name: orders_user_idx; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX orders_user_idx ON ONLY public.orders USING btree (user_id);


--
-- Name: orders_2026_user_id_idx; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX orders_2026_user_id_idx ON public.orders_2026 USING btree (user_id);


--
-- Name: orders_2026_pkey; Type: INDEX ATTACH; Schema: public; Owner: postgres
--

ALTER INDEX public.orders_pkey ATTACH PARTITION public.orders_2026_pkey;


--
-- Name: orders_2026_user_id_idx; Type: INDEX ATTACH; Schema: public; Owner: postgres
--

ALTER INDEX public.orders_user_idx ATTACH PARTITION public.orders_2026_user_id_idx;


--
-- Name: users_stats; Type: STATISTICS; Schema: public; Owner: postgres
--

CREATE STATISTICS public.users_stats ON id, email FROM public.users;


ALTER STATISTICS public.users_stats OWNER TO postgres;

--
-- Name: orders orders_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE public.orders
    ADD CONSTRAINT orders_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id);


--
-- Name: shop_pub; Type: PUBLICATION; Schema: -; Owner: postgres
--

CREATE PUBLICATION shop_pub WITH (publish = 'insert, update, delete, truncate');


ALTER PUBLICATION shop_pub OWNER TO postgres;

--
-- Name: shop_pub users; Type: PUBLICATION TABLE; Schema: public; Owner: postgres
--

ALTER PUBLICATION shop_pub ADD TABLE ONLY public.users;


--
-- Name: log_ddl; Type: EVENT TRIGGER; Schema: -; Owner: postgres
--

CREATE EVENT TRIGGER log_ddl ON ddl_command_start
   EXECUTE FUNCTION public.on_ddl();


ALTER EVENT TRIGGER log_ddl OWNER TO postgres;

--
-- Name: TABLE users; Type: SECURITY LABEL; Schema: public; Owner: postgres
--

SECURITY LABEL FOR selinux ON TABLE public.users IS 'system_u:object_r:sepgsql_table_t:s0';


--
-- PostgreSQL database dump complete
--

\unrestrict fixture

