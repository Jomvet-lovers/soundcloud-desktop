CREATE TABLE "enrich_calibration" (
	"source" varchar(16) NOT NULL,
	"raw_bin_low" real NOT NULL,
	"raw_bin_high" real NOT NULL,
	"calibrated" real NOT NULL,
	"sample_count" integer NOT NULL DEFAULT 0,
	"updated_at" timestamp with time zone NOT NULL DEFAULT now(),
	PRIMARY KEY ("source", "raw_bin_low", "raw_bin_high"),
	CHECK ("raw_bin_low" >= 0 AND "raw_bin_high" <= 1 AND "raw_bin_low" < "raw_bin_high"),
	CHECK ("calibrated" >= 0 AND "calibrated" <= 1)
);
CREATE INDEX "enrich_calibration_source_idx" ON "enrich_calibration" ("source");
