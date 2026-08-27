-- Role values are present in the bootstrap users table. This migration makes
-- the low-privilege default and constraint explicit for all new accounts.
ALTER TABLE users
    ALTER COLUMN role SET DEFAULT 'Requester';

ALTER TABLE users
    DROP CONSTRAINT IF EXISTS users_role_check;

ALTER TABLE users
    ADD CONSTRAINT users_role_check
    CHECK (role IN ('Owner', 'Admin', 'RequirementManager', 'Requester'));
