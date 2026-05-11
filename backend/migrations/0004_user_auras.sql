CREATE TABLE "user_auras" (
	"user_urn" text PRIMARY KEY NOT NULL,
	"aura_id" varchar(32) NOT NULL,
	"custom_hex" varchar(7),
	"updated_at" timestamp with time zone NOT NULL DEFAULT now()
);
