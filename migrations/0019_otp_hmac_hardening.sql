-- Invalidate verification codes written before keyed OTP storage was deployed.
-- Rows remain for audit/history; only active legacy codes are consumed.
UPDATE verification_codes
SET used_at = CURRENT_TIMESTAMP
WHERE used_at IS NULL
  AND expires_at > CURRENT_TIMESTAMP;
