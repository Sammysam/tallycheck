-- D84: users sign in with their email (or, as before, their username). The email is optional so
-- existing users keep working; when given it is unique, whatever its letter case.
ALTER TABLE "users" ADD COLUMN "email" TEXT;
CREATE UNIQUE INDEX "UQ_users_email" ON "users" (lower("email")) WHERE "email" IS NOT NULL;
