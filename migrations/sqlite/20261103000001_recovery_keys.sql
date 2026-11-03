-- D49: password recovery keys. Each user may have one key ("TCRK-XXXX-…", 100 random bits),
-- shown once; only its SHA-256 is stored. Using it to reset the password consumes it and
-- issues a new one.
CREATE TABLE "recovery_keys" (
    "userId" TEXT PRIMARY KEY NOT NULL REFERENCES "users" ("id"),
    "keyHash" TEXT NOT NULL,
    "createdAt" TEXT NOT NULL
) STRICT;
