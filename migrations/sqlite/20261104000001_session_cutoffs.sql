-- D52: ending a user's sign-ins. Sign-in tokens (JWTs) issued before "validFrom" (Unix seconds,
-- compared with the token's `iat`) are refused. Set when an administrator resets a user's
-- password or 2FA, when a password is reset with a recovery key or Windows sign-in, and when a
-- user is switched off. A separate table, so "users" stays identical to the reference.
CREATE TABLE "session_cutoffs" (
    "userId" TEXT PRIMARY KEY NOT NULL REFERENCES "users" ("id"),
    "validFrom" INTEGER NOT NULL CHECK ("validFrom" >= 0)
) STRICT;
