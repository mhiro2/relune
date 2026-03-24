-- Broken SQL Input
-- This file intentionally contains various SQL errors for parser testing

-- ============================================
-- SYNTAX ERRORS
-- ============================================

-- Missing closing parenthesis
CREATE TABLE broken_syntax_1 (
    id SERIAL PRIMARY KEY,
    name VARCHAR(255)
    -- missing closing paren

-- Invalid keyword
CREATE TABLE broken_syntax_2 (
    id SERIAL PRIMARY KEY,
    INVALID_KEYWORD VARCHAR(255)
);

-- Missing comma between columns
CREATE TABLE broken_syntax_3 (
    id SERIAL PRIMARY KEY
    name VARCHAR(255)
);

-- Invalid type
CREATE TABLE broken_syntax_4 (
    id SERIAL PRIMARY KEY,
    value NOT_A_REAL_TYPE
);

-- ============================================
-- INVALID CONSTRAINT REFERENCES
-- ============================================

-- Foreign key references non-existent table
CREATE TABLE invalid_fk_1 (
    id SERIAL PRIMARY KEY,
    user_id INTEGER REFERENCES nonexistent_table(id)
);

-- Foreign key references non-existent column
CREATE TABLE invalid_fk_2 (
    id SERIAL PRIMARY KEY,
    order_id INTEGER REFERENCES orders(nonexistent_column)
);

-- Self-referential FK to wrong column
CREATE TABLE invalid_fk_3 (
    id SERIAL PRIMARY KEY,
    parent_id INTEGER REFERENCES invalid_fk_3(wrong_column)
);

-- Circular FK with invalid syntax in reference
CREATE TABLE invalid_fk_4 (
    id SERIAL PRIMARY KEY,
    other_id INTEGER REFERENCES "invalid..name"(id)
);

-- ============================================
-- DUPLICATE TABLE DEFINITIONS
-- ============================================

-- First definition
CREATE TABLE duplicate_table (
    id SERIAL PRIMARY KEY,
    name VARCHAR(100)
);

-- Second definition with same name (should cause error)
CREATE TABLE duplicate_table (
    id SERIAL PRIMARY KEY,
    title VARCHAR(100),
    description TEXT
);

-- Duplicate with different case (may or may not be error depending on DB)
CREATE TABLE DUPLICATE_TABLE (
    id SERIAL PRIMARY KEY,
    value INTEGER
);

-- ============================================
-- MALFORMED COLUMN TYPES
-- ============================================

-- Missing type entirely
CREATE TABLE malformed_type_1 (
    id SERIAL PRIMARY KEY,
    value
);

-- Invalid varchar length
CREATE TABLE malformed_type_2 (
    id SERIAL PRIMARY KEY,
    name VARCHAR(-1)
);

-- Invalid decimal precision
CREATE TABLE malformed_type_3 (
    id SERIAL PRIMARY KEY,
    amount DECIMAL(0, 0)
);

-- Nonsense type with parameters
CREATE TABLE malformed_type_4 (
    id SERIAL PRIMARY KEY,
    data INTEGER(255, 100, 'weird')
);

-- ============================================
-- INVALID CONSTRAINT SYNTAX
-- ============================================

-- Invalid CHECK constraint
CREATE TABLE broken_check (
    id SERIAL PRIMARY KEY,
    value INTEGER CHECK
);

-- Invalid UNIQUE syntax
CREATE TABLE broken_unique (
    id SERIAL PRIMARY KEY,
    email VARCHAR(255) UNIQUE ON
);

-- Invalid DEFAULT value
CREATE TABLE broken_default (
    id SERIAL PRIMARY KEY,
    created_at TIMESTAMP DEFAULT NOT_VALID_FUNCTION()
);

-- ============================================
-- MISCELLANEOUS ERRORS
-- ============================================

-- Empty table definition
CREATE TABLE empty_table ();

-- Index on non-existent table
CREATE INDEX idx_nonexistent ON nonexistent_table(column_name);

-- Index on non-existent column
CREATE TABLE has_index_target (
    id SERIAL PRIMARY KEY
);
CREATE INDEX idx_missing_column ON has_index_target(missing_column);

-- Duplicate index name
CREATE TABLE index_test (
    id SERIAL PRIMARY KEY,
    value VARCHAR(100)
);
CREATE INDEX idx_duplicate_name ON index_test(value);
CREATE INDEX idx_duplicate_name ON index_test(id);

-- Invalid schema-qualified table reference
CREATE TABLE "invalid..schema".bad_table (
    id SERIAL PRIMARY KEY
);

-- Unclosed string literal
CREATE TABLE unclosed_string (
    id SERIAL PRIMARY KEY,
    name VARCHAR(255) DEFAULT 'unclosed string
);
