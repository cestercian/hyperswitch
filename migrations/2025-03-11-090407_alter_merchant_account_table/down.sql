-- This file should undo anything in `up.sql`
ALTER TABLE merchant_account
  ALTER COLUMN webhook_details
  SET DATA TYPE JSON