-- Discover Spotlight: курируемые промо-карточки + настройки fallback'а.

CREATE TABLE "discover_promoted" (
    "id" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
    "entity_type" varchar(8) NOT NULL,
    "entity_id" uuid NOT NULL,
    "position" integer NOT NULL DEFAULT 0,
    "active" boolean NOT NULL DEFAULT true,
    "note" text,
    "created_at" timestamp with time zone NOT NULL DEFAULT now(),
    "updated_at" timestamp with time zone NOT NULL DEFAULT now(),
    CHECK (entity_type IN ('artist', 'album'))
);

CREATE UNIQUE INDEX "discover_promoted_entity_uq"
    ON "discover_promoted" ("entity_type", "entity_id");

CREATE INDEX "discover_promoted_order_idx"
    ON "discover_promoted" ("active", "position", "created_at")
    WHERE active = TRUE;

CREATE TABLE "discover_settings" (
    "id" smallint PRIMARY KEY DEFAULT 1 NOT NULL,
    "show_star" boolean NOT NULL DEFAULT true,
    "star_strategy" varchar(16) NOT NULL DEFAULT 'popular',
    "star_limit" integer NOT NULL DEFAULT 8,
    "updated_at" timestamp with time zone NOT NULL DEFAULT now(),
    CHECK (id = 1),
    CHECK (star_strategy IN ('popular', 'random')),
    CHECK (star_limit BETWEEN 0 AND 24)
);

INSERT INTO "discover_settings" ("id") VALUES (1) ON CONFLICT DO NOTHING;
