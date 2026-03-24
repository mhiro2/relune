-- Join-Heavy SaaS Multi-Tenant Schema
-- Typical SaaS application with many many-to-many relationships

-- ============================================
-- TENANT AND AUTH
-- ============================================

-- Tenants (organizations/accounts)
CREATE TABLE tenants (
    id SERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    slug VARCHAR(100) NOT NULL UNIQUE,
    plan VARCHAR(50) NOT NULL DEFAULT 'free',
    is_active BOOLEAN NOT NULL DEFAULT true,
    settings JSONB DEFAULT '{}',
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- Users (belongs to tenant)
CREATE TABLE users (
    id SERIAL PRIMARY KEY,
    tenant_id INTEGER NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    email VARCHAR(255) NOT NULL,
    password_hash VARCHAR(255) NOT NULL,
    first_name VARCHAR(100),
    last_name VARCHAR(100),
    is_active BOOLEAN NOT NULL DEFAULT true,
    last_login_at TIMESTAMP WITH TIME ZONE,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (tenant_id, email)
);

-- Roles (global roles per tenant)
CREATE TABLE roles (
    id SERIAL PRIMARY KEY,
    tenant_id INTEGER NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    name VARCHAR(100) NOT NULL,
    description TEXT,
    is_system BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (tenant_id, name)
);

-- User-Role join table (many-to-many)
CREATE TABLE user_roles (
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role_id INTEGER NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    assigned_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    assigned_by INTEGER REFERENCES users(id) ON DELETE SET NULL,
    PRIMARY KEY (user_id, role_id)
);

-- Permissions
CREATE TABLE permissions (
    id SERIAL PRIMARY KEY,
    name VARCHAR(100) NOT NULL UNIQUE,
    resource VARCHAR(100) NOT NULL,
    action VARCHAR(50) NOT NULL,
    description TEXT
);

-- Role-Permission join table (many-to-many)
CREATE TABLE role_permissions (
    role_id INTEGER NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    permission_id INTEGER NOT NULL REFERENCES permissions(id) ON DELETE CASCADE,
    PRIMARY KEY (role_id, permission_id)
);

-- ============================================
-- CORE ENTITIES
-- ============================================

-- Projects
CREATE TABLE projects (
    id SERIAL PRIMARY KEY,
    tenant_id INTEGER NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    description TEXT,
    status VARCHAR(50) NOT NULL DEFAULT 'active',
    start_date DATE,
    end_date DATE,
    created_by INTEGER REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- Teams
CREATE TABLE teams (
    id SERIAL PRIMARY KEY,
    tenant_id INTEGER NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    description TEXT,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- Project-Team join table (many-to-many)
CREATE TABLE project_teams (
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    team_id INTEGER NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    access_level VARCHAR(50) NOT NULL DEFAULT 'read',
    added_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (project_id, team_id)
);

-- User-Team join table (many-to-many)
CREATE TABLE team_members (
    team_id INTEGER NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role VARCHAR(50) NOT NULL DEFAULT 'member',
    joined_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (team_id, user_id)
);

-- Tags (for categorizing various entities)
CREATE TABLE tags (
    id SERIAL PRIMARY KEY,
    tenant_id INTEGER NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    name VARCHAR(100) NOT NULL,
    color VARCHAR(7) DEFAULT '#808080',
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (tenant_id, name)
);

-- ============================================
-- TASKS AND WORK ITEMS
-- ============================================

-- Tasks
CREATE TABLE tasks (
    id SERIAL PRIMARY KEY,
    tenant_id INTEGER NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    assignee_id INTEGER REFERENCES users(id) ON DELETE SET NULL,
    reporter_id INTEGER REFERENCES users(id) ON DELETE SET NULL,
    title VARCHAR(500) NOT NULL,
    description TEXT,
    status VARCHAR(50) NOT NULL DEFAULT 'todo',
    priority VARCHAR(50) NOT NULL DEFAULT 'medium',
    due_date DATE,
    estimated_hours DECIMAL(6, 2),
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- Task-Tag join table (many-to-many)
CREATE TABLE task_tags (
    task_id INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    PRIMARY KEY (task_id, tag_id)
);

-- Task dependencies (self-referential many-to-many)
CREATE TABLE task_dependencies (
    task_id INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    depends_on_task_id INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    dependency_type VARCHAR(50) NOT NULL DEFAULT 'blocks',
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (task_id, depends_on_task_id)
);

-- ============================================
-- COMMENTS AND ATTACHMENTS
-- ============================================

-- Comments (polymorphic - can belong to tasks, projects, etc.)
CREATE TABLE comments (
    id SERIAL PRIMARY KEY,
    tenant_id INTEGER NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    author_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    commentable_type VARCHAR(50) NOT NULL,
    commentable_id INTEGER NOT NULL,
    content TEXT NOT NULL,
    parent_id INTEGER REFERENCES comments(id) ON DELETE CASCADE,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- Attachments
CREATE TABLE attachments (
    id SERIAL PRIMARY KEY,
    tenant_id INTEGER NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    uploader_id INTEGER NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    attachable_type VARCHAR(50) NOT NULL,
    attachable_id INTEGER NOT NULL,
    filename VARCHAR(255) NOT NULL,
    content_type VARCHAR(100),
    file_size BIGINT NOT NULL,
    storage_path VARCHAR(500) NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- ============================================
-- NOTIFICATIONS AND ACTIVITY
-- ============================================

-- Notifications
CREATE TABLE notifications (
    id SERIAL PRIMARY KEY,
    tenant_id INTEGER NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    notification_type VARCHAR(100) NOT NULL,
    title VARCHAR(255) NOT NULL,
    content TEXT,
    related_type VARCHAR(50),
    related_id INTEGER,
    is_read BOOLEAN NOT NULL DEFAULT false,
    read_at TIMESTAMP WITH TIME ZONE,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- Activity log
CREATE TABLE activities (
    id SERIAL PRIMARY KEY,
    tenant_id INTEGER NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    actor_id INTEGER REFERENCES users(id) ON DELETE SET NULL,
    activity_type VARCHAR(100) NOT NULL,
    subject_type VARCHAR(50),
    subject_id INTEGER,
    metadata JSONB DEFAULT '{}',
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- ============================================
-- WATCHERS/FOLLOWERS
-- ============================================

-- Task watchers (many-to-many)
CREATE TABLE task_watchers (
    task_id INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    watching_since TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (task_id, user_id)
);

-- Project watchers (many-to-many)
CREATE TABLE project_watchers (
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    watching_since TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (project_id, user_id)
);

-- ============================================
-- INTEGRATIONS
-- ============================================

-- Integration types (system-wide)
CREATE TABLE integration_types (
    id SERIAL PRIMARY KEY,
    name VARCHAR(100) NOT NULL UNIQUE,
    display_name VARCHAR(255) NOT NULL,
    description TEXT,
    icon_url VARCHAR(500),
    config_schema JSONB
);

-- Tenant integrations
CREATE TABLE integrations (
    id SERIAL PRIMARY KEY,
    tenant_id INTEGER NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    integration_type_id INTEGER NOT NULL REFERENCES integration_types(id) ON DELETE RESTRICT,
    name VARCHAR(255) NOT NULL,
    config JSONB DEFAULT '{}',
    credentials_encrypted TEXT,
    is_active BOOLEAN NOT NULL DEFAULT true,
    last_sync_at TIMESTAMP WITH TIME ZONE,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (tenant_id, name)
);

-- Project integrations (link specific projects to integrations)
CREATE TABLE project_integrations (
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    integration_id INTEGER NOT NULL REFERENCES integrations(id) ON DELETE CASCADE,
    external_project_id VARCHAR(255),
    sync_enabled BOOLEAN NOT NULL DEFAULT true,
    config JSONB DEFAULT '{}',
    PRIMARY KEY (project_id, integration_id)
);

-- ============================================
-- INDEXES
-- ============================================

-- Tenant indexes
CREATE INDEX idx_tenants_slug ON tenants(slug);
CREATE INDEX idx_tenants_plan ON tenants(plan);

-- User indexes
CREATE INDEX idx_users_tenant_id ON users(tenant_id);
CREATE INDEX idx_users_email ON users(email);
CREATE INDEX idx_users_tenant_email ON users(tenant_id, email);

-- Role indexes
CREATE INDEX idx_roles_tenant_id ON roles(tenant_id);
CREATE INDEX idx_user_roles_user_id ON user_roles(user_id);
CREATE INDEX idx_user_roles_role_id ON user_roles(role_id);
CREATE INDEX idx_role_permissions_role_id ON role_permissions(role_id);
CREATE INDEX idx_role_permissions_perm_id ON role_permissions(permission_id);

-- Project indexes
CREATE INDEX idx_projects_tenant_id ON projects(tenant_id);
CREATE INDEX idx_projects_status ON projects(status);
CREATE INDEX idx_projects_created_by ON projects(created_by);

-- Team indexes
CREATE INDEX idx_teams_tenant_id ON teams(tenant_id);
CREATE INDEX idx_project_teams_project ON project_teams(project_id);
CREATE INDEX idx_project_teams_team ON project_teams(team_id);
CREATE INDEX idx_team_members_team ON team_members(team_id);
CREATE INDEX idx_team_members_user ON team_members(user_id);

-- Task indexes
CREATE INDEX idx_tasks_tenant_id ON tasks(tenant_id);
CREATE INDEX idx_tasks_project_id ON tasks(project_id);
CREATE INDEX idx_tasks_assignee ON tasks(assignee_id);
CREATE INDEX idx_tasks_status ON tasks(status);
CREATE INDEX idx_tasks_priority ON tasks(priority);
CREATE INDEX idx_tasks_due_date ON tasks(due_date);
CREATE INDEX idx_task_tags_task ON task_tags(task_id);
CREATE INDEX idx_task_tags_tag ON task_tags(tag_id);
CREATE INDEX idx_task_deps_task ON task_dependencies(task_id);
CREATE INDEX idx_task_deps_depends ON task_dependencies(depends_on_task_id);

-- Comment indexes
CREATE INDEX idx_comments_tenant ON comments(tenant_id);
CREATE INDEX idx_comments_author ON comments(author_id);
CREATE INDEX idx_comments_commentable ON comments(commentable_type, commentable_id);

-- Attachment indexes
CREATE INDEX idx_attachments_tenant ON attachments(tenant_id);
CREATE INDEX idx_attachments_uploader ON attachments(uploader_id);
CREATE INDEX idx_attachments_attachable ON attachments(attachable_type, attachable_id);

-- Notification indexes
CREATE INDEX idx_notifications_user ON notifications(user_id);
CREATE INDEX idx_notifications_unread ON notifications(user_id, is_read) WHERE is_read = false;

-- Activity indexes
CREATE INDEX idx_activities_tenant ON activities(tenant_id);
CREATE INDEX idx_activities_actor ON activities(actor_id);
CREATE INDEX idx_activities_subject ON activities(subject_type, subject_id);
CREATE INDEX idx_activities_created ON activities(created_at);

-- Watcher indexes
CREATE INDEX idx_task_watchers_task ON task_watchers(task_id);
CREATE INDEX idx_task_watchers_user ON task_watchers(user_id);
CREATE INDEX idx_project_watchers_project ON project_watchers(project_id);
CREATE INDEX idx_project_watchers_user ON project_watchers(user_id);

-- Integration indexes
CREATE INDEX idx_integrations_tenant ON integrations(tenant_id);
CREATE INDEX idx_integrations_type ON integrations(integration_type_id);
CREATE INDEX idx_project_integrations_project ON project_integrations(project_id);
CREATE INDEX idx_project_integrations_integration ON project_integrations(integration_id);

-- Tag indexes
CREATE INDEX idx_tags_tenant ON tags(tenant_id);
