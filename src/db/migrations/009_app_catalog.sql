CREATE TABLE IF NOT EXISTS app_catalog (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    app_name    TEXT NOT NULL UNIQUE,
    category    TEXT NOT NULL,
    icon        TEXT NOT NULL,
    vendor      TEXT,
    homepage    TEXT
);

CREATE TABLE IF NOT EXISTS app_domains (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    app_id      INTEGER NOT NULL REFERENCES app_catalog(id) ON DELETE CASCADE,
    domain      TEXT NOT NULL UNIQUE
);

CREATE INDEX IF NOT EXISTS idx_app_domains_domain ON app_domains(domain);

-- Seed data: app_catalog
INSERT OR IGNORE INTO app_catalog (app_name, category, icon, vendor, homepage) VALUES
-- Streaming
('YouTube', 'Streaming', '🎬', 'Google', 'https://youtube.com'),
('Netflix', 'Streaming', '🎬', 'Netflix', 'https://netflix.com'),
('Spotify', 'Streaming', '🎵', 'Spotify', 'https://spotify.com'),
('Disney+', 'Streaming', '🏰', 'Disney', 'https://disneyplus.com'),
('TikTok', 'Streaming', '🎵', 'ByteDance', 'https://tiktok.com'),
('Twitch', 'Streaming', '🎮', 'Amazon', 'https://twitch.tv'),
('Bilibili', 'Streaming', '📺', 'Bilibili', 'https://bilibili.com'),
('爱奇艺', 'Streaming', '📺', 'Baidu', 'https://iqiyi.com'),
('优酷', 'Streaming', '📺', 'Alibaba', 'https://youku.com'),
('Prime Video', 'Streaming', '📦', 'Amazon', 'https://primevideo.com'),
('Apple TV+', 'Streaming', '🍎', 'Apple', 'https://tv.apple.com'),
-- Social
('Facebook', 'Social', '👥', 'Meta', 'https://facebook.com'),
('Instagram', 'Social', '📸', 'Meta', 'https://instagram.com'),
('Twitter/X', 'Social', '🐦', 'X Corp', 'https://x.com'),
('LinkedIn', 'Social', '💼', 'Microsoft', 'https://linkedin.com'),
('Reddit', 'Social', '🤖', 'Reddit', 'https://reddit.com'),
('微信', 'Social', '💬', 'Tencent', 'https://weixin.qq.com'),
('微博', 'Social', '🌐', 'Sina', 'https://weibo.com'),
('抖音', 'Social', '🎵', 'ByteDance', 'https://douyin.com'),
('Line', 'Social', '💬', 'Line', 'https://line.me'),
('Pinterest', 'Social', '📌', 'Pinterest', 'https://pinterest.com'),
-- Tech/Cloud
('Google', 'Tech', '🔍', 'Google', 'https://google.com'),
('Apple', 'Tech', '🍎', 'Apple', 'https://apple.com'),
('Microsoft', 'Tech', '🪟', 'Microsoft', 'https://microsoft.com'),
('Amazon AWS', 'Tech', '☁️', 'Amazon', 'https://aws.amazon.com'),
('Cloudflare', 'Tech', '☁️', 'Cloudflare', 'https://cloudflare.com'),
('GitHub', 'Tech', '🐙', 'Microsoft', 'https://github.com'),
('Dropbox', 'Tech', '📦', 'Dropbox', 'https://dropbox.com'),
-- Gaming
('Steam', 'Gaming', '🎮', 'Valve', 'https://steampowered.com'),
('Epic Games', 'Gaming', '🎮', 'Epic', 'https://epicgames.com'),
('PlayStation', 'Gaming', '🎮', 'Sony', 'https://playstation.com'),
('Xbox', 'Gaming', '🎮', 'Microsoft', 'https://xbox.com'),
('Nintendo', 'Gaming', '🎮', 'Nintendo', 'https://nintendo.com'),
('Riot Games', 'Gaming', '⚔️', 'Riot', 'https://riotgames.com'),
-- Communication
('Zoom', 'Communication', '📹', 'Zoom', 'https://zoom.us'),
('Slack', 'Communication', '💬', 'Salesforce', 'https://slack.com'),
('Teams', 'Communication', '💼', 'Microsoft', 'https://teams.microsoft.com'),
('Discord', 'Communication', '🎮', 'Discord', 'https://discord.com'),
('Telegram', 'Communication', '✈️', 'Telegram', 'https://telegram.org'),
('WhatsApp', 'Communication', '💬', 'Meta', 'https://whatsapp.com'),
('Skype', 'Communication', '📞', 'Microsoft', 'https://skype.com'),
-- Shopping
('Amazon', 'Shopping', '📦', 'Amazon', 'https://amazon.com'),
('淘宝/天猫', 'Shopping', '🛍️', 'Alibaba', 'https://taobao.com'),
('京东', 'Shopping', '🛒', 'JD', 'https://jd.com'),
('Shopify', 'Shopping', '🛍️', 'Shopify', 'https://shopify.com'),
('eBay', 'Shopping', '🏷️', 'eBay', 'https://ebay.com'),
-- Finance
('PayPal', 'Finance', '💳', 'PayPal', 'https://paypal.com'),
('Stripe', 'Finance', '💳', 'Stripe', 'https://stripe.com'),
('支付宝', 'Finance', '💰', 'Ant Group', 'https://alipay.com'),
-- News
('BBC', 'News', '📰', 'BBC', 'https://bbc.com'),
('CNN', 'News', '📰', 'CNN', 'https://cnn.com');

-- Seed data: app_domains
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'youtube.com' FROM app_catalog WHERE app_name = 'YouTube';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'googlevideo.com' FROM app_catalog WHERE app_name = 'YouTube';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'ytimg.com' FROM app_catalog WHERE app_name = 'YouTube';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'yt3.ggpht.com' FROM app_catalog WHERE app_name = 'YouTube';

INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'netflix.com' FROM app_catalog WHERE app_name = 'Netflix';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'nflxvideo.net' FROM app_catalog WHERE app_name = 'Netflix';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'nflxext.com' FROM app_catalog WHERE app_name = 'Netflix';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'nflxso.net' FROM app_catalog WHERE app_name = 'Netflix';

INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'spotify.com' FROM app_catalog WHERE app_name = 'Spotify';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'spotifycdn.com' FROM app_catalog WHERE app_name = 'Spotify';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'scdn.co' FROM app_catalog WHERE app_name = 'Spotify';

INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'disneyplus.com' FROM app_catalog WHERE app_name = 'Disney+';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'bamgrid.com' FROM app_catalog WHERE app_name = 'Disney+';

INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'tiktok.com' FROM app_catalog WHERE app_name = 'TikTok';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'tiktokcdn.com' FROM app_catalog WHERE app_name = 'TikTok';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'musical.ly' FROM app_catalog WHERE app_name = 'TikTok';

INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'twitch.tv' FROM app_catalog WHERE app_name = 'Twitch';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'twitchsvc.net' FROM app_catalog WHERE app_name = 'Twitch';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'jtvnw.net' FROM app_catalog WHERE app_name = 'Twitch';

INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'bilibili.com' FROM app_catalog WHERE app_name = 'Bilibili';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'bilivideo.com' FROM app_catalog WHERE app_name = 'Bilibili';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'hdslb.com' FROM app_catalog WHERE app_name = 'Bilibili';

INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'iqiyi.com' FROM app_catalog WHERE app_name = '爱奇艺';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'qiyi.com' FROM app_catalog WHERE app_name = '爱奇艺';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'iqiyipic.com' FROM app_catalog WHERE app_name = '爱奇艺';

INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'youku.com' FROM app_catalog WHERE app_name = '优酷';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'alikunlun.com' FROM app_catalog WHERE app_name = '优酷';

INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'primevideo.com' FROM app_catalog WHERE app_name = 'Prime Video';

INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'tv.apple.com' FROM app_catalog WHERE app_name = 'Apple TV+';

-- Social
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'facebook.com' FROM app_catalog WHERE app_name = 'Facebook';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'fbcdn.net' FROM app_catalog WHERE app_name = 'Facebook';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'fb.com' FROM app_catalog WHERE app_name = 'Facebook';

INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'instagram.com' FROM app_catalog WHERE app_name = 'Instagram';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'cdninstagram.com' FROM app_catalog WHERE app_name = 'Instagram';

INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'twitter.com' FROM app_catalog WHERE app_name = 'Twitter/X';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'x.com' FROM app_catalog WHERE app_name = 'Twitter/X';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'twimg.com' FROM app_catalog WHERE app_name = 'Twitter/X';

INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'linkedin.com' FROM app_catalog WHERE app_name = 'LinkedIn';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'licdn.com' FROM app_catalog WHERE app_name = 'LinkedIn';

INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'reddit.com' FROM app_catalog WHERE app_name = 'Reddit';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'redditmedia.com' FROM app_catalog WHERE app_name = 'Reddit';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'redd.it' FROM app_catalog WHERE app_name = 'Reddit';

INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'weixin.qq.com' FROM app_catalog WHERE app_name = '微信';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'wechat.com' FROM app_catalog WHERE app_name = '微信';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'wx.qq.com' FROM app_catalog WHERE app_name = '微信';

INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'weibo.com' FROM app_catalog WHERE app_name = '微博';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'sinaimg.cn' FROM app_catalog WHERE app_name = '微博';

INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'douyin.com' FROM app_catalog WHERE app_name = '抖音';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'douyinpic.com' FROM app_catalog WHERE app_name = '抖音';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'pstatp.com' FROM app_catalog WHERE app_name = '抖音';

INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'line.me' FROM app_catalog WHERE app_name = 'Line';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'line-scdn.net' FROM app_catalog WHERE app_name = 'Line';

INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'pinterest.com' FROM app_catalog WHERE app_name = 'Pinterest';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'pinimg.com' FROM app_catalog WHERE app_name = 'Pinterest';

-- Tech
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'google.com' FROM app_catalog WHERE app_name = 'Google';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'googleapis.com' FROM app_catalog WHERE app_name = 'Google';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'gstatic.com' FROM app_catalog WHERE app_name = 'Google';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'googleusercontent.com' FROM app_catalog WHERE app_name = 'Google';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'googlesyndication.com' FROM app_catalog WHERE app_name = 'Google';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'googleadservices.com' FROM app_catalog WHERE app_name = 'Google';

INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'apple.com' FROM app_catalog WHERE app_name = 'Apple';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'icloud.com' FROM app_catalog WHERE app_name = 'Apple';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'mzstatic.com' FROM app_catalog WHERE app_name = 'Apple';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'apple-cloudkit.com' FROM app_catalog WHERE app_name = 'Apple';

INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'microsoft.com' FROM app_catalog WHERE app_name = 'Microsoft';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'msn.com' FROM app_catalog WHERE app_name = 'Microsoft';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'live.com' FROM app_catalog WHERE app_name = 'Microsoft';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'windows.net' FROM app_catalog WHERE app_name = 'Microsoft';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'azure.com' FROM app_catalog WHERE app_name = 'Microsoft';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'microsoftonline.com' FROM app_catalog WHERE app_name = 'Microsoft';

INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'amazonaws.com' FROM app_catalog WHERE app_name = 'Amazon AWS';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'aws.amazon.com' FROM app_catalog WHERE app_name = 'Amazon AWS';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'cloudfront.net' FROM app_catalog WHERE app_name = 'Amazon AWS';

INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'cloudflare.com' FROM app_catalog WHERE app_name = 'Cloudflare';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'cloudflare-dns.com' FROM app_catalog WHERE app_name = 'Cloudflare';

INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'github.com' FROM app_catalog WHERE app_name = 'GitHub';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'githubusercontent.com' FROM app_catalog WHERE app_name = 'GitHub';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'githubassets.com' FROM app_catalog WHERE app_name = 'GitHub';

INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'dropbox.com' FROM app_catalog WHERE app_name = 'Dropbox';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'dropboxstatic.com' FROM app_catalog WHERE app_name = 'Dropbox';

-- Gaming
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'steampowered.com' FROM app_catalog WHERE app_name = 'Steam';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'steamcontent.com' FROM app_catalog WHERE app_name = 'Steam';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'steamstatic.com' FROM app_catalog WHERE app_name = 'Steam';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'akamaihd.net' FROM app_catalog WHERE app_name = 'Steam';

INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'epicgames.com' FROM app_catalog WHERE app_name = 'Epic Games';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'unrealengine.com' FROM app_catalog WHERE app_name = 'Epic Games';

INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'playstation.com' FROM app_catalog WHERE app_name = 'PlayStation';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'playstation.net' FROM app_catalog WHERE app_name = 'PlayStation';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'dl.delivery.mp.microsoft.com' FROM app_catalog WHERE app_name = 'PlayStation';

INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'xbox.com' FROM app_catalog WHERE app_name = 'Xbox';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'xboxlive.com' FROM app_catalog WHERE app_name = 'Xbox';

INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'nintendo.com' FROM app_catalog WHERE app_name = 'Nintendo';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'nintendo.net' FROM app_catalog WHERE app_name = 'Nintendo';

INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'riotgames.com' FROM app_catalog WHERE app_name = 'Riot Games';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'leagueoflegends.com' FROM app_catalog WHERE app_name = 'Riot Games';

-- Communication
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'zoom.us' FROM app_catalog WHERE app_name = 'Zoom';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'zoomgov.com' FROM app_catalog WHERE app_name = 'Zoom';

INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'slack.com' FROM app_catalog WHERE app_name = 'Slack';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'slack-edge.com' FROM app_catalog WHERE app_name = 'Slack';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'slack-msgs.com' FROM app_catalog WHERE app_name = 'Slack';

INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'teams.microsoft.com' FROM app_catalog WHERE app_name = 'Teams';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'teams.live.com' FROM app_catalog WHERE app_name = 'Teams';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'skype.com' FROM app_catalog WHERE app_name = 'Skype';

INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'discord.com' FROM app_catalog WHERE app_name = 'Discord';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'discordapp.com' FROM app_catalog WHERE app_name = 'Discord';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'discord.gg' FROM app_catalog WHERE app_name = 'Discord';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'discordcdn.com' FROM app_catalog WHERE app_name = 'Discord';

INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'telegram.org' FROM app_catalog WHERE app_name = 'Telegram';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 't.me' FROM app_catalog WHERE app_name = 'Telegram';

INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'whatsapp.com' FROM app_catalog WHERE app_name = 'WhatsApp';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'whatsapp.net' FROM app_catalog WHERE app_name = 'WhatsApp';

-- Shopping
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'amazon.com' FROM app_catalog WHERE app_name = 'Amazon';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'amazon.co.jp' FROM app_catalog WHERE app_name = 'Amazon';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'amazon.co.uk' FROM app_catalog WHERE app_name = 'Amazon';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'amazon.cn' FROM app_catalog WHERE app_name = 'Amazon';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'images-amazon.com' FROM app_catalog WHERE app_name = 'Amazon';

INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'taobao.com' FROM app_catalog WHERE app_name = '淘宝/天猫';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'tmall.com' FROM app_catalog WHERE app_name = '淘宝/天猫';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'alicdn.com' FROM app_catalog WHERE app_name = '淘宝/天猫';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'tbcdn.cn' FROM app_catalog WHERE app_name = '淘宝/天猫';

INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'jd.com' FROM app_catalog WHERE app_name = '京东';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'jdpic.net' FROM app_catalog WHERE app_name = '京东';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'jcloudcs.com' FROM app_catalog WHERE app_name = '京东';

INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'shopify.com' FROM app_catalog WHERE app_name = 'Shopify';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'shopifycdn.com' FROM app_catalog WHERE app_name = 'Shopify';

INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'ebay.com' FROM app_catalog WHERE app_name = 'eBay';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'ebayimg.com' FROM app_catalog WHERE app_name = 'eBay';

-- Finance
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'paypal.com' FROM app_catalog WHERE app_name = 'PayPal';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'paypalobjects.com' FROM app_catalog WHERE app_name = 'PayPal';

INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'stripe.com' FROM app_catalog WHERE app_name = 'Stripe';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'stripe.network' FROM app_catalog WHERE app_name = 'Stripe';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'stripecdn.com' FROM app_catalog WHERE app_name = 'Stripe';

INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'alipay.com' FROM app_catalog WHERE app_name = '支付宝';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'alipayobjects.com' FROM app_catalog WHERE app_name = '支付宝';

-- News
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'bbc.com' FROM app_catalog WHERE app_name = 'BBC';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'bbc.co.uk' FROM app_catalog WHERE app_name = 'BBC';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'bbci.co.uk' FROM app_catalog WHERE app_name = 'BBC';

INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'cnn.com' FROM app_catalog WHERE app_name = 'CNN';
INSERT OR IGNORE INTO app_domains (app_id, domain)
SELECT id, 'turner.com' FROM app_catalog WHERE app_name = 'CNN';
