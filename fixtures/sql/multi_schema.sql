-- Multi-Schema PostgreSQL Schema
-- Demonstrates cross-schema foreign keys and schema organization

-- Schema: public (default user-facing data)
CREATE SCHEMA IF NOT EXISTS public;

-- Schema: inventory (product and stock management)
CREATE SCHEMA IF NOT EXISTS inventory;

-- Schema: sales (orders and transactions)
CREATE SCHEMA IF NOT EXISTS sales;

-- ============================================
-- PUBLIC SCHEMA
-- ============================================

-- Users table in public schema
CREATE TABLE public.users (
    id SERIAL PRIMARY KEY,
    email VARCHAR(255) NOT NULL UNIQUE,
    name VARCHAR(255) NOT NULL,
    role VARCHAR(50) NOT NULL DEFAULT 'customer',
    is_active BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- Posts table in public schema
CREATE TABLE public.posts (
    id SERIAL PRIMARY KEY,
    author_id INTEGER NOT NULL REFERENCES public.users(id) ON DELETE RESTRICT,
    title VARCHAR(500) NOT NULL,
    content TEXT,
    status VARCHAR(50) NOT NULL DEFAULT 'draft',
    published_at TIMESTAMP WITH TIME ZONE,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- ============================================
-- INVENTORY SCHEMA
-- ============================================

-- Products table in inventory schema
CREATE TABLE inventory.products (
    id SERIAL PRIMARY KEY,
    sku VARCHAR(100) NOT NULL UNIQUE,
    name VARCHAR(500) NOT NULL,
    description TEXT,
    unit_cost DECIMAL(10, 2) NOT NULL CHECK (unit_cost >= 0),
    reorder_level INTEGER NOT NULL DEFAULT 10,
    is_discontinued BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- Stock table in inventory schema
CREATE TABLE inventory.stock (
    id SERIAL PRIMARY KEY,
    product_id INTEGER NOT NULL REFERENCES inventory.products(id) ON DELETE RESTRICT,
    warehouse_code VARCHAR(20) NOT NULL,
    quantity_on_hand INTEGER NOT NULL DEFAULT 0 CHECK (quantity_on_hand >= 0),
    quantity_reserved INTEGER NOT NULL DEFAULT 0 CHECK (quantity_reserved >= 0),
    last_counted_at TIMESTAMP WITH TIME ZONE,
    UNIQUE (product_id, warehouse_code)
);

-- ============================================
-- SALES SCHEMA
-- ============================================

-- Orders table in sales schema (references public.users)
CREATE TABLE sales.orders (
    id SERIAL PRIMARY KEY,
    customer_id INTEGER NOT NULL REFERENCES public.users(id) ON DELETE RESTRICT,
    order_number VARCHAR(50) NOT NULL UNIQUE,
    status VARCHAR(50) NOT NULL DEFAULT 'pending',
    total_amount DECIMAL(10, 2) NOT NULL CHECK (total_amount >= 0),
    order_date TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    shipped_at TIMESTAMP WITH TIME ZONE,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- Order items table (references sales.orders and inventory.products)
CREATE TABLE sales.order_items (
    id SERIAL PRIMARY KEY,
    order_id INTEGER NOT NULL REFERENCES sales.orders(id) ON DELETE CASCADE,
    product_id INTEGER NOT NULL REFERENCES inventory.products(id) ON DELETE RESTRICT,
    quantity INTEGER NOT NULL CHECK (quantity > 0),
    unit_price DECIMAL(10, 2) NOT NULL CHECK (unit_price >= 0),
    line_total DECIMAL(10, 2) NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- Shipments table (references sales.orders and inventory.stock)
CREATE TABLE sales.shipments (
    id SERIAL PRIMARY KEY,
    order_id INTEGER NOT NULL REFERENCES sales.orders(id) ON DELETE RESTRICT,
    warehouse_code VARCHAR(20) NOT NULL,
    tracking_number VARCHAR(100),
    carrier VARCHAR(100),
    status VARCHAR(50) NOT NULL DEFAULT 'pending',
    shipped_at TIMESTAMP WITH TIME ZONE,
    delivered_at TIMESTAMP WITH TIME ZONE,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- Indexes
CREATE INDEX idx_public_users_email ON public.users(email);
CREATE INDEX idx_public_posts_author_id ON public.posts(author_id);
CREATE INDEX idx_public_posts_status ON public.posts(status);

CREATE INDEX idx_inventory_products_sku ON inventory.products(sku);
CREATE INDEX idx_inventory_stock_product_id ON inventory.stock(product_id);
CREATE INDEX idx_inventory_stock_warehouse ON inventory.stock(warehouse_code);

CREATE INDEX idx_sales_orders_customer_id ON sales.orders(customer_id);
CREATE INDEX idx_sales_orders_status ON sales.orders(status);
CREATE INDEX idx_sales_orders_order_date ON sales.orders(order_date);
CREATE INDEX idx_sales_order_items_order_id ON sales.order_items(order_id);
CREATE INDEX idx_sales_order_items_product_id ON sales.order_items(product_id);
CREATE INDEX idx_sales_shipments_order_id ON sales.shipments(order_id);
