-- Cyclic Foreign Keys Schema
-- Demonstrates self-referential and cyclic foreign key relationships

-- ============================================
-- SELF-REFERENTIAL FOREIGN KEYS
-- ============================================

-- Employees with manager relationship (classic self-referential FK)
CREATE TABLE employees (
    id SERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    email VARCHAR(255) NOT NULL UNIQUE,
    manager_id INTEGER REFERENCES employees(id) ON DELETE SET NULL,
    department VARCHAR(100),
    hire_date DATE NOT NULL DEFAULT CURRENT_DATE,
    salary DECIMAL(12, 2) CHECK (salary > 0),
    is_active BOOLEAN NOT NULL DEFAULT true
);

-- Categories with parent/child hierarchy
CREATE TABLE categories (
    id SERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    slug VARCHAR(255) NOT NULL UNIQUE,
    parent_id INTEGER REFERENCES categories(id) ON DELETE RESTRICT,
    level INTEGER NOT NULL DEFAULT 0,
    path VARCHAR(1000),
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- Organizations with hierarchical structure
CREATE TABLE organizations (
    id SERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    parent_org_id INTEGER REFERENCES organizations(id) ON DELETE SET NULL,
    org_type VARCHAR(50) NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- Comments with threading (parent comment)
CREATE TABLE threaded_comments (
    id SERIAL PRIMARY KEY,
    post_id INTEGER NOT NULL,
    parent_comment_id INTEGER REFERENCES threaded_comments(id) ON DELETE CASCADE,
    author_name VARCHAR(255) NOT NULL,
    content TEXT NOT NULL,
    depth INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- ============================================
-- SIMPLE CYCLIC FK CHAIN: a -> b -> c -> a
-- ============================================

-- Table A references C
CREATE TABLE cycle_a (
    id SERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    cycle_c_id INTEGER REFERENCES cycle_c(id) ON DELETE RESTRICT,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- Table B references A
CREATE TABLE cycle_b (
    id SERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    cycle_a_id INTEGER REFERENCES cycle_a(id) ON DELETE RESTRICT,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- Table C references B (completing the cycle)
CREATE TABLE cycle_c (
    id SERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    cycle_b_id INTEGER REFERENCES cycle_b(id) ON DELETE RESTRICT,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- ============================================
-- COMPLEX CYCLE: departments <-> projects
-- ============================================

-- Departments with lead project
CREATE TABLE departments (
    id SERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL UNIQUE,
    lead_project_id INTEGER,  -- Forward reference to projects
    budget DECIMAL(15, 2) NOT NULL DEFAULT 0,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- Projects with owning department
CREATE TABLE projects (
    id SERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    department_id INTEGER NOT NULL REFERENCES departments(id) ON DELETE RESTRICT,
    status VARCHAR(50) NOT NULL DEFAULT 'planning',
    start_date DATE,
    end_date DATE,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- Add the circular reference constraint to departments
ALTER TABLE departments
    ADD CONSTRAINT fk_departments_lead_project
    FOREIGN KEY (lead_project_id) REFERENCES projects(id) ON DELETE SET NULL;

-- ============================================
-- TRIANGLE CYCLE: users -> teams -> orgs -> users
-- ============================================

-- Users table
CREATE TABLE users (
    id SERIAL PRIMARY KEY,
    email VARCHAR(255) NOT NULL UNIQUE,
    name VARCHAR(255) NOT NULL,
    default_team_id INTEGER,  -- Forward reference
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- Teams table
CREATE TABLE teams (
    id SERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    organization_id INTEGER NOT NULL,  -- Forward reference
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- Organizations table
CREATE TABLE organizations_ref (
    id SERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    owner_user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- Add delayed FK constraints
ALTER TABLE users
    ADD CONSTRAINT fk_users_default_team
    FOREIGN KEY (default_team_id) REFERENCES teams(id) ON DELETE SET NULL;

ALTER TABLE teams
    ADD CONSTRAINT fk_teams_organization
    FOREIGN KEY (organization_id) REFERENCES organizations_ref(id) ON DELETE CASCADE;

-- ============================================
-- INDEXES
-- ============================================

CREATE INDEX idx_employees_manager_id ON employees(manager_id);
CREATE INDEX idx_employees_department ON employees(department);
CREATE INDEX idx_categories_parent_id ON categories(parent_id);
CREATE INDEX idx_categories_slug ON categories(slug);
CREATE INDEX idx_organizations_parent ON organizations(parent_org_id);
CREATE INDEX idx_threaded_comments_parent ON threaded_comments(parent_comment_id);
CREATE INDEX idx_threaded_comments_post ON threaded_comments(post_id);
CREATE INDEX idx_cycle_a_ref ON cycle_a(cycle_c_id);
CREATE INDEX idx_cycle_b_ref ON cycle_b(cycle_a_id);
CREATE INDEX idx_cycle_c_ref ON cycle_c(cycle_b_id);
CREATE INDEX idx_projects_department ON projects(department_id);
CREATE INDEX idx_departments_lead_project ON departments(lead_project_id);
