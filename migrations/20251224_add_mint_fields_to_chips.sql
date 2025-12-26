-- Migration: Add mint-related fields to chips table
-- Date: 2025-12-24
-- Description: Add is_mint and mint_user fields to support chip recycling for userMint transactions

-- Add is_mint field (0: not minted, 1: minting, 2: minted/recycled)
ALTER TABLE chips 
ADD COLUMN IF NOT EXISTS is_mint INTEGER DEFAULT 0 NOT NULL;

-- Add mint_user field (address of user who minted this chip)
ALTER TABLE chips 
ADD COLUMN IF NOT EXISTS mint_user VARCHAR(42);

-- Add comment for is_mint field
COMMENT ON COLUMN chips.is_mint IS 'Mint status: 0=not minted, 1=minting, 2=minted/recycled';

-- Add comment for mint_user field
COMMENT ON COLUMN chips.mint_user IS 'Address of user who minted this chip (set when is_mint=2)';

-- Create index for querying minted chips
CREATE INDEX IF NOT EXISTS idx_chips_is_mint ON chips(is_mint);

-- Create index for querying chips by mint_user
CREATE INDEX IF NOT EXISTS idx_chips_mint_user ON chips(mint_user);

-- Create composite index for efficient queries
CREATE INDEX IF NOT EXISTS idx_chips_is_mint_mint_user ON chips(is_mint, mint_user);

