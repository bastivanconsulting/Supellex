-- parc-it : appareils réseau (VyOS, Cisco, MikroTik) — pilotage SSH / API
-- PostgreSQL 17

CREATE TYPE device_platform AS ENUM ('vyos', 'cisco', 'mikrotik');
CREATE TYPE device_transport AS ENUM ('ssh', 'api');

CREATE TABLE devices (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name             TEXT NOT NULL,
    host             TEXT NOT NULL,
    port             INTEGER NOT NULL DEFAULT 22,
    platform         device_platform NOT NULL,
    transport        device_transport NOT NULL DEFAULT 'ssh',
    username         TEXT NOT NULL,
    -- Secret chiffré AES-256-GCM (nonce 12 o || chiffré+tag), clé DEVICES_ENCRYPTION_KEY.
    -- Jamais renvoyé par l'API.
    secret_cipher    TEXT NOT NULL,
    secret_kind      TEXT NOT NULL CHECK (secret_kind IN ('password', 'ssh_key')),
    -- Interface physique portant les VLANs (ex. eth0, ether1).
    uplink_interface TEXT,
    -- TOFU : empreinte SHA256 de la clé publique SSH épinglée au premier contact.
    host_key_fp      TEXT,
    last_test_at     TIMESTAMPTZ,
    last_test_ok     BOOLEAN,
    last_sync_at     TIMESTAMPTZ,
    last_sync_ok     BOOLEAN,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX devices_host_idx ON devices (host);
