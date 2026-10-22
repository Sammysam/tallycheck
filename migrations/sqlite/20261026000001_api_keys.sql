-- D41: keys for Claude (MCP) and other local programs. A key acts as the user who created it,
-- with that user's permissions; read-only unless "canWrite" lets it create drafts. Only a SHA-256
-- hash of the key is stored; the key itself is shown once, when it is created.
CREATE TABLE "api_keys" (
    "id" TEXT PRIMARY KEY NOT NULL,
    "name" TEXT NOT NULL CHECK (length(trim("name")) > 0),
    "userId" TEXT NOT NULL REFERENCES "users" ("id"),
    "prefix" TEXT NOT NULL,
    "keyHash" TEXT NOT NULL UNIQUE,
    "canWrite" INTEGER NOT NULL DEFAULT 0 CHECK ("canWrite" IN (0, 1)),
    "createdAt" TEXT NOT NULL,
    "lastUsedAt" TEXT,
    "revokedAt" TEXT
) STRICT;
