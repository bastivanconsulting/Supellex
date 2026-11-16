-- parc-it : schéma initial (ITAM + IPAM + auth + audit)
-- PostgreSQL 17

CREATE EXTENSION IF NOT EXISTS "pgcrypto";
CREATE EXTENSION IF NOT EXISTS "citext";

-- ============================================================
-- Authentification & droits
-- ============================================================

CREATE TYPE user_role AS ENUM ('admin', 'operator', 'viewer');

CREATE TABLE users (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    email         CITEXT NOT NULL UNIQUE,
    display_name  TEXT NOT NULL,
    password_hash TEXT NOT NULL,              -- argon2id
    role          user_role NOT NULL DEFAULT 'viewer',
    must_change_password BOOLEAN NOT NULL DEFAULT FALSE,
    disabled      BOOLEAN NOT NULL DEFAULT FALSE,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_login_at TIMESTAMPTZ
);

-- ============================================================
-- Annuaire interne (personnes à qui on assigne du matériel)
-- ============================================================

CREATE TABLE people (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    first_name TEXT NOT NULL,
    last_name  TEXT NOT NULL,
    email      CITEXT UNIQUE,
    phone      TEXT,
    department TEXT,
    job_title  TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_people_name ON people (last_name, first_name);

-- ============================================================
-- Sites / salles / baies (arborescence d'emplacement)
-- ============================================================

CREATE TABLE locations (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name        TEXT NOT NULL,
    kind        TEXT NOT NULL DEFAULT 'site' CHECK (kind IN ('site', 'building', 'room', 'rack')),
    parent_id   UUID REFERENCES locations(id) ON DELETE SET NULL,
    address     TEXT,
    description TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (parent_id, name)
);

CREATE INDEX idx_locations_parent ON locations (parent_id);

-- ============================================================
-- Parc informatique (actifs matériels / logiciels)
-- ============================================================

CREATE TYPE asset_type AS ENUM (
    'laptop', 'desktop', 'server', 'switch', 'router', 'firewall',
    'access_point', 'printer', 'phone', 'monitor', 'nas', 'ups',
    'appliance', 'license', 'other'
);

CREATE TYPE asset_status AS ENUM ('in_stock', 'deployed', 'repair', 'retired', 'lost');

CREATE TABLE assets (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    asset_tag     TEXT NOT NULL UNIQUE,        -- étiquette d'inventaire
    type          asset_type NOT NULL,
    status        asset_status NOT NULL DEFAULT 'in_stock',
    manufacturer  TEXT,
    model         TEXT,
    serial_number TEXT,
    hostname      TEXT,
    os            TEXT,
    purchase_date DATE,
    warranty_end  DATE,
    purchase_price NUMERIC(12,2),
    location_id   UUID REFERENCES locations(id) ON DELETE SET NULL,
    assigned_to   UUID REFERENCES people(id) ON DELETE SET NULL,
    notes         TEXT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_assets_type ON assets (type);
CREATE INDEX idx_assets_status ON assets (status);
CREATE INDEX idx_assets_assigned ON assets (assigned_to) WHERE assigned_to IS NOT NULL;
CREATE INDEX idx_assets_location ON assets (location_id) WHERE location_id IS NOT NULL;

-- Historique d'assignation d'un actif à une personne
CREATE TABLE asset_assignments (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    asset_id   UUID NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    person_id  UUID REFERENCES people(id) ON DELETE SET NULL,
    assigned_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    returned_at TIMESTAMPTZ,
    note       TEXT
);

CREATE INDEX idx_assignments_asset ON asset_assignments (asset_id, assigned_at DESC);

-- ============================================================
-- IPAM
-- ============================================================

-- Espaces d'adressage (VRF / tenant / zone)
CREATE TABLE ip_spaces (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name        TEXT NOT NULL UNIQUE,
    description TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- VLANs
CREATE TABLE vlans (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    vid        INTEGER NOT NULL CHECK (vid BETWEEN 1 AND 4094),
    name       TEXT NOT NULL,
    description TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (vid)
);

-- Sous-réseaux CIDR
CREATE TABLE subnets (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    space_id     UUID NOT NULL REFERENCES ip_spaces(id) ON DELETE CASCADE,
    cidr         INET NOT NULL,
    name         TEXT,
    description  TEXT,
    gateway      INET,
    vlan_id      UUID REFERENCES vlans(id) ON DELETE SET NULL,
    location_id  UUID REFERENCES locations(id) ON DELETE SET NULL,
    dns_servers  INET[],
    is_pool      BOOLEAN NOT NULL DEFAULT FALSE,  -- plage réservée aux attributions
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (space_id, cidr)
);

CREATE INDEX idx_subnets_space ON subnets (space_id);

-- Adresses IP individuelles
CREATE TYPE ip_status AS ENUM ('free', 'allocated', 'reserved', 'dhcp', 'gateway');

CREATE TABLE ip_addresses (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    subnet_id   UUID NOT NULL REFERENCES subnets(id) ON DELETE CASCADE,
    address     INET NOT NULL,
    status      ip_status NOT NULL DEFAULT 'free',
    hostname    TEXT,
    mac         TEXT CHECK (mac IS NULL OR mac ~ '^([0-9A-Fa-f]{2}:){5}[0-9A-Fa-f]{2}$'),
    asset_id    UUID REFERENCES assets(id) ON DELETE SET NULL,
    person_id   UUID REFERENCES people(id) ON DELETE SET NULL,
    description TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (subnet_id, address)
);

CREATE UNIQUE INDEX idx_ip_unique_global ON ip_addresses (address) WHERE status <> 'free';
CREATE INDEX idx_ip_subnet ON ip_addresses (subnet_id);
CREATE INDEX idx_ip_status ON ip_addresses (status);
CREATE INDEX idx_ip_asset ON ip_addresses (asset_id) WHERE asset_id IS NOT NULL;

-- ============================================================
-- Audit : toute mutation tracée (ISO 27001 A.8.15 / NIS2 art. 21)
-- ============================================================

CREATE TABLE audit_log (
    id           BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    actor_id     UUID REFERENCES users(id) ON DELETE SET NULL,
    actor_email  TEXT,                          -- conservé même si l'utilisateur est supprimé
    action       TEXT NOT NULL,                 -- ex: asset.update, ip.allocate
    entity_type  TEXT NOT NULL,                 -- ex: asset, subnet, ip_address
    entity_id    TEXT,
    details      JSONB NOT NULL DEFAULT '{}'::jsonb,
    ip           INET,
    correlation_id UUID
);

CREATE INDEX idx_audit_at ON audit_log (at DESC);
CREATE INDEX idx_audit_entity ON audit_log (entity_type, entity_id);
CREATE INDEX idx_audit_actor ON audit_log (actor_id);

-- ============================================================
-- Trigger updated_at
-- ============================================================

CREATE OR REPLACE FUNCTION set_updated_at() RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = now();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DO $$
DECLARE t TEXT;
BEGIN
    FOR t IN SELECT unnest(ARRAY['users','people','locations','assets','ip_spaces','subnets','ip_addresses'])
    LOOP
        EXECUTE format('CREATE TRIGGER trg_%I_updated BEFORE UPDATE ON %I FOR EACH ROW EXECUTE FUNCTION set_updated_at()', t, t);
    END LOOP;
END $$;
