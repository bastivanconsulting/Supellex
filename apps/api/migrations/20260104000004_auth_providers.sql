-- parc-it : sources d'authentification externes (LDAP, SSO/OIDC)
-- PostgreSQL 17
--
-- La source « local » (table users, argon2id) existe toujours. Cette table
-- décrit les fournisseurs externes configurables par un admin : annuaire
-- LDAP (bind service + recherche puis bind utilisateur) et fournisseurs
-- OIDC (flux authorization code + PKCE).

CREATE TYPE auth_provider_kind AS ENUM ('ldap', 'oidc');

CREATE TABLE auth_providers (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Identifiant stable utilisé dans l'URL et le login (ex. "ldap-ad").
    slug        TEXT NOT NULL UNIQUE CHECK (slug ~ '^[a-z0-9][a-z0-9-]{1,62}$'),
    label       TEXT NOT NULL CHECK (char_length(label) BETWEEN 1 AND 100),
    kind        auth_provider_kind NOT NULL,
    enabled     BOOLEAN NOT NULL DEFAULT TRUE,
    -- Configuration JSONB propre à chaque kind. Les champs secrets
    -- (bind_password LDAP, client_secret OIDC) y sont stockés chiffrés
    -- AES-256-GCM avec préfixe "enc:" (même clé que les secrets devices,
    -- AAD distinct) et ne sont jamais renvoyés par l'API.
    config      JSONB NOT NULL DEFAULT '{}'::jsonb,
    last_test_at  TIMESTAMPTZ,
    last_test_ok  BOOLEAN,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TRIGGER trg_auth_providers_updated
    BEFORE UPDATE ON auth_providers
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- Sessions de login OIDC en cours (state -> provider + PKCE + nonce).
-- Supprimées à l'utilisation (one-shot) ou à l'expiration.
CREATE TABLE oidc_login_sessions (
    state         TEXT PRIMARY KEY,
    provider_id   UUID NOT NULL REFERENCES auth_providers(id) ON DELETE CASCADE,
    nonce         TEXT NOT NULL,
    pkce_verifier TEXT NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at    TIMESTAMPTZ NOT NULL
);

CREATE INDEX oidc_login_sessions_exp ON oidc_login_sessions (expires_at);

-- Source d'auth du compte : 'local' (défaut) ou slug d'un auth_provider.
-- Les comptes provisionnés par LDAP/OIDC ont un hash de mot de passe
-- aléatoire inutilisable : le login local ne peut pas matcher.
ALTER TABLE users ADD COLUMN auth_source TEXT NOT NULL DEFAULT 'local';
