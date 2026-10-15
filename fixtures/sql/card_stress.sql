-- Card Stress Schema
-- Exercises node cards with long identifiers, Japanese identifiers,
-- wide tables, composite keys, and a mix of nullable and indexed columns.

-- ============================================
-- LONG IDENTIFIERS
-- ============================================

CREATE TABLE organization_membership_invitations (
    id BIGSERIAL PRIMARY KEY,
    organization_identifier BIGINT NOT NULL,
    invited_email_address_normalized VARCHAR(320) NOT NULL,
    invitation_acceptance_deadline_at TIMESTAMP WITH TIME ZONE NOT NULL,
    accepted_at TIMESTAMP WITH TIME ZONE,
    revoked_by_administrator_user_id BIGINT,
    custom_onboarding_message TEXT
);

CREATE INDEX idx_invitations_email
    ON organization_membership_invitations (invited_email_address_normalized);

-- ============================================
-- JAPANESE IDENTIFIERS
-- ============================================

CREATE TABLE "顧客" (
    "顧客ID" BIGSERIAL PRIMARY KEY,
    "氏名" VARCHAR(100) NOT NULL,
    "フリガナ" VARCHAR(200),
    "メールアドレス" VARCHAR(320) NOT NULL UNIQUE,
    "電話番号" VARCHAR(20),
    "登録日時" TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE "注文" (
    "注文ID" BIGSERIAL PRIMARY KEY,
    "顧客ID" BIGINT NOT NULL REFERENCES "顧客" ("顧客ID"),
    "注文日" DATE NOT NULL,
    "合計金額" NUMERIC(12, 2) NOT NULL,
    "備考" TEXT
);

CREATE INDEX "注文_顧客ID_idx" ON "注文" ("顧客ID");

-- ============================================
-- COMPOSITE KEYS
-- ============================================

CREATE TABLE warehouses (
    region_code CHAR(2) NOT NULL,
    warehouse_number INTEGER NOT NULL,
    display_name VARCHAR(120) NOT NULL,
    PRIMARY KEY (region_code, warehouse_number)
);

CREATE TABLE warehouse_bins (
    region_code CHAR(2) NOT NULL,
    warehouse_number INTEGER NOT NULL,
    bin_label VARCHAR(16) NOT NULL,
    capacity_units INTEGER,
    PRIMARY KEY (region_code, warehouse_number, bin_label),
    FOREIGN KEY (region_code, warehouse_number)
        REFERENCES warehouses (region_code, warehouse_number)
);

CREATE TABLE stock_movements (
    id BIGSERIAL PRIMARY KEY,
    region_code CHAR(2) NOT NULL,
    warehouse_number INTEGER NOT NULL,
    bin_label VARCHAR(16) NOT NULL,
    quantity_delta INTEGER NOT NULL,
    moved_at TIMESTAMP WITH TIME ZONE NOT NULL,
    FOREIGN KEY (region_code, warehouse_number, bin_label)
        REFERENCES warehouse_bins (region_code, warehouse_number, bin_label)
);

CREATE INDEX idx_stock_movements_bin
    ON stock_movements (region_code, warehouse_number, bin_label);

-- ============================================
-- WIDE TABLE
-- ============================================

CREATE TABLE product_catalog_entries (
    id BIGSERIAL PRIMARY KEY,
    sku VARCHAR(64) NOT NULL UNIQUE,
    title VARCHAR(255) NOT NULL,
    subtitle VARCHAR(255),
    description TEXT,
    brand VARCHAR(120),
    manufacturer VARCHAR(120),
    country_of_origin CHAR(2),
    category_path TEXT NOT NULL,
    list_price NUMERIC(12, 2) NOT NULL,
    sale_price NUMERIC(12, 2),
    cost_price NUMERIC(12, 2),
    currency CHAR(3) NOT NULL,
    tax_class VARCHAR(32),
    weight_grams INTEGER,
    width_mm INTEGER,
    height_mm INTEGER,
    depth_mm INTEGER,
    color VARCHAR(40),
    size_label VARCHAR(40),
    material VARCHAR(80),
    barcode_ean13 CHAR(13),
    is_published BOOLEAN NOT NULL DEFAULT false,
    is_discontinued BOOLEAN NOT NULL DEFAULT false,
    published_at TIMESTAMP WITH TIME ZONE,
    discontinued_at TIMESTAMP WITH TIME ZONE,
    search_keywords TEXT[],
    attributes JSONB NOT NULL DEFAULT '{}',
    default_region_code CHAR(2),
    default_warehouse_number INTEGER,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (default_region_code, default_warehouse_number)
        REFERENCES warehouses (region_code, warehouse_number)
);

CREATE INDEX idx_catalog_published ON product_catalog_entries (is_published, published_at);
