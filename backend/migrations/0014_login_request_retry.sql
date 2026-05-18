ALTER TABLE "login_requests" ADD COLUMN "retry_count" integer DEFAULT 0 NOT NULL;--> statement-breakpoint
ALTER TABLE "login_requests" ADD COLUMN "redirect_url" text;
