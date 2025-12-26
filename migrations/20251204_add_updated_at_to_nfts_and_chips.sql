-- Add updated_at column to nfts table
ALTER TABLE nfts ADD COLUMN updated_at TIMESTAMPTZ DEFAULT NOW();

-- Add updated_at column to chips table
ALTER TABLE chips ADD COLUMN updated_at TIMESTAMPTZ DEFAULT NOW();

-- Create a function to automatically update updated_at
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ language 'plpgsql';

-- Create triggers to update updated_at on change
CREATE TRIGGER update_nfts_updated_at
    BEFORE UPDATE ON nfts
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_chips_updated_at
    BEFORE UPDATE ON chips
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at_column();
