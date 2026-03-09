-- Parental Control protection levels
-- Add settings for parental control level
INSERT INTO settings (key, value) VALUES
    ('parental_control_level', 'none')
ON CONFLICT DO NOTHING;

-- Parental Control preset categories
-- These are built-in domain lists organized by content category
-- Used when parental_control_enabled is true and level is set
CREATE TABLE IF NOT EXISTS parental_control_categories (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    description TEXT,
    level       TEXT NOT NULL CHECK (level IN ('basic','standard','strict')),
    domains     TEXT NOT NULL,  -- JSON array of domains
    created_at  TEXT NOT NULL DEFAULT (NOW()::TEXT)
);

-- Insert built-in preset categories
-- BASIC: Malware and phishing only
INSERT INTO parental_control_categories (id, name, description, level, domains) VALUES
    ('basic-malware', 'Malware Sites',
     'Known malware distribution sites', 'basic',
     '["malware.example.com","phishing.example.com"]'),
    ('basic-phishing', 'Phishing Sites',
     'Known phishing and scam sites', 'basic',
     '["phishing.com","scam.com"]')
ON CONFLICT DO NOTHING;

-- STANDARD: Adult content, gambling, drugs
INSERT INTO parental_control_categories (id, name, description, level, domains) VALUES
    ('std-adult', 'Adult Content',
     'Adult and pornographic websites', 'standard',
     '["pornhub.com","xvideos.com","xnxx.com","redtube.com",
      "youjizz.com","tube8.com","spankbang.com"]'),
    ('std-gambling', 'Gambling',
     'Online gambling and betting sites', 'standard',
     '["poker.com","casino.com","bet365.com","888.com","williamhill.com"]'),
    ('std-drugs', 'Drugs',
     'Illegal drugs and substance sites', 'standard',
     '["drugs.com","buydrugs.com","weed.com","marijuana.com"]')
ON CONFLICT DO NOTHING;

-- STRICT: Social media, gaming, streaming (in addition to standard)
INSERT INTO parental_control_categories (id, name, description, level, domains) VALUES
    ('strict-social', 'Social Media',
     'Social networking platforms', 'strict',
     '["facebook.com","twitter.com","instagram.com","tiktok.com",
      "snapchat.com","linkedin.com","reddit.com"]'),
    ('strict-gaming', 'Gaming',
     'Online gaming platforms', 'strict',
     '["steam.com","epicgames.com","blizzard.com","playstation.com",
      "xbox.com","nintendo.com","roblox.com"]'),
    ('strict-streaming', 'Streaming',
     'Video streaming services', 'strict',
     '["netflix.com","hulu.com","disneyplus.com","hbo.com",
      "amazon.com/video","primevideo.com"]')
ON CONFLICT DO NOTHING;
