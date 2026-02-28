-- Migration 003: Security fixes
-- L-1: Add UNIQUE constraint on dns_rewrites.domain to prevent duplicate rewrites
-- Using a unique index (compatible with both empty and populated databases)
CREATE UNIQUE INDEX IF NOT EXISTS idx_dns_rewrites_domain ON dns_rewrites(domain);
