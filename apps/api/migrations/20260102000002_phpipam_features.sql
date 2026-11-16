-- parc-it : incrément "fonctions PHPIPAM" — domaines VLAN, sous-réseaux
-- imbriqués, découverte réseau (ping/ARP), tags, plages DHCP, NAT,
-- champs personnalisés, statut "offline".
-- PostgreSQL 17

-- Nouveau statut : l'IP ne répond plus au ping (découverte).
-- (ALTER TYPE ADD VALUE est autorisé en transaction depuis PG 12 ; la valeur
--  n'est utilisable qu'après commit, donc pas dans ce fichier.)
ALTER TYPE ip_status ADD VALUE IF NOT EXISTS 'offline';

-- ============================================================
-- Domaines VLAN (PHPIPAM : un VID est unique par domaine)
-- ============================================================

CREATE TABLE vlan_domains (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name        TEXT NOT NULL UNIQUE,
    description TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

INSERT INTO vlan_domains (name, description)
VALUES ('Default', 'Domaine VLAN par défaut')
ON CONFLICT DO NOTHING;

ALTER TABLE vlans ADD COLUMN vlan_domain_id UUID REFERENCES vlan_domains(id) ON DELETE RESTRICT;
UPDATE vlans SET vlan_domain_id = (SELECT id FROM vlan_domains WHERE name = 'Default');
ALTER TABLE vlans ALTER COLUMN vlan_domain_id SET NOT NULL;
ALTER TABLE vlans DROP CONSTRAINT vlans_vid_key;
CREATE UNIQUE INDEX idx_vlans_domain_vid ON vlans (vlan_domain_id, vid);

-- ============================================================
-- Sous-réseaux imbriqués (master subnet) + découverte
-- ============================================================

ALTER TABLE subnets ADD COLUMN master_subnet_id UUID REFERENCES subnets(id) ON DELETE SET NULL;
ALTER TABLE subnets ADD COLUMN ping_scan BOOLEAN NOT NULL DEFAULT TRUE;  -- participe au balayage ping auto
ALTER TABLE subnets ADD COLUMN custom_fields JSONB NOT NULL DEFAULT '{}'::jsonb;
CREATE INDEX idx_subnets_master ON subnets (master_subnet_id);

-- ============================================================
-- Adresses IP : horodatage ping + champs personnalisés
-- ============================================================

ALTER TABLE ip_addresses ADD COLUMN last_seen TIMESTAMPTZ;
ALTER TABLE ip_addresses ADD COLUMN last_ping_ok BOOLEAN;
ALTER TABLE ip_addresses ADD COLUMN last_ping_at TIMESTAMPTZ;
ALTER TABLE ip_addresses ADD COLUMN custom_fields JSONB NOT NULL DEFAULT '{}'::jsonb;
CREATE INDEX idx_ip_mac ON ip_addresses (mac) WHERE mac IS NOT NULL;

-- ============================================================
-- Tags (étiquettes sur les IP, extensible à d'autres entités)
-- ============================================================

CREATE TABLE tags (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name       CITEXT NOT NULL UNIQUE,
    color      TEXT NOT NULL DEFAULT '#0e7490' CHECK (color ~ '^#[0-9A-Fa-f]{6}$'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE ip_tags (
    ip_id  UUID NOT NULL REFERENCES ip_addresses(id) ON DELETE CASCADE,
    tag_id UUID NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    PRIMARY KEY (ip_id, tag_id)
);

-- ============================================================
-- Plages DHCP (PHPIPAM : pools d'adresses distribuées par DHCP)
-- ============================================================

CREATE TABLE dhcp_ranges (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    subnet_id   UUID NOT NULL REFERENCES subnets(id) ON DELETE CASCADE,
    start_addr  INET NOT NULL,
    end_addr    INET NOT NULL,
    name        TEXT,
    description TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (start_addr <= end_addr)
);

CREATE INDEX idx_dhcp_subnet ON dhcp_ranges (subnet_id);

-- ============================================================
-- NAT (règles simples src -> dst)
-- ============================================================

CREATE TABLE nat_rules (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name          TEXT NOT NULL,
    src_subnet_id UUID REFERENCES subnets(id) ON DELETE CASCADE,
    src_address   INET,
    dst_address   INET,
    description   TEXT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (src_address IS NOT NULL OR src_subnet_id IS NOT NULL)
);

-- ============================================================
-- Définitions de champs personnalisés (PHPIPAM custom fields)
-- ============================================================

CREATE TABLE field_defs (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    entity     TEXT NOT NULL CHECK (entity IN ('ip_address', 'subnet', 'asset')),
    name       TEXT NOT NULL,
    field_type TEXT NOT NULL DEFAULT 'text' CHECK (field_type IN ('text', 'number', 'date', 'select')),
    options    JSONB NOT NULL DEFAULT '[]'::jsonb,  -- valeurs possibles si select
    required   BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (entity, name)
);
