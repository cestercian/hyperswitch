-- This file should undo anything in `up.sql`

ALTER TABLE payment_methods DROP COLUMN IF EXISTS token_locker_id;