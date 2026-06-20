CREATE INDEX "idx_campaign_members_user_id" ON "campaign_members" ("user_id");

CREATE INDEX "idx_campaigns_owner_user_id" ON "campaigns" ("owner_user_id");

CREATE TABLE "campaign_members" ( "campaign_id" varchar NOT NULL, "user_id" uuid_text NOT NULL, "role" varchar NOT NULL CHECK ("role" IN ('gm', 'player')), "created_at" timestamp_with_timezone_text NOT NULL, PRIMARY KEY ("campaign_id", "user_id"), FOREIGN KEY ("campaign_id") REFERENCES "campaigns" ("id"), FOREIGN KEY ("user_id") REFERENCES "users" ("id") );

CREATE TABLE "campaigns" ( "id" varchar NOT NULL PRIMARY KEY, "owner_user_id" uuid_text NOT NULL, "shard_url" varchar NOT NULL, "name" varchar NULL, "tagline" varchar NULL, "game_system" varchar NULL, "content_locale" varchar NULL, "last_init_error" varchar NULL, "wizard_completed_at" timestamp_with_timezone_text NULL, "created_at" timestamp_with_timezone_text NOT NULL, "updated_at" timestamp_with_timezone_text NOT NULL, FOREIGN KEY ("owner_user_id") REFERENCES "users" ("id") );

CREATE TABLE "create_attempts" ( "idempotency_token" varchar NOT NULL PRIMARY KEY, "campaign_id" varchar NOT NULL, "created_at" timestamp_with_timezone_text NOT NULL );

CREATE TABLE "users" ( "id" uuid_text NOT NULL PRIMARY KEY, "email" varchar NOT NULL UNIQUE, "created_at" timestamp_with_timezone_text NOT NULL, "updated_at" timestamp_with_timezone_text NOT NULL );
