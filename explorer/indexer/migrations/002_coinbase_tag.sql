-- Add coinbase_tag column for operator-configured block identification
ALTER TABLE blocks ADD COLUMN IF NOT EXISTS coinbase_tag TEXT;
