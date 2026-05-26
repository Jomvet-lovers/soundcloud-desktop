-- `country` парсится из SC payload как `country_code` (короткий ISO типа "US")
-- с fallback на `country` (полное название — "United Kingdom" и длиннее).
-- Изначальный varchar(8) рассчитан только на ISO-код и роняет UPSERT юзеров,
-- у которых SC отдал длинное имя страны.

ALTER TABLE users ALTER COLUMN country TYPE varchar(64);
