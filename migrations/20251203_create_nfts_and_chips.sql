-- Drop tables if they exist to ensure schema consistency
DROP TABLE IF EXISTS chips;
DROP TABLE IF EXISTS nfts;

-- Create nfts table
CREATE TABLE nfts (
    id SERIAL PRIMARY KEY,
    user_address VARCHAR(255),
    received BOOLEAN DEFAULT FALSE,
    created_at TIMESTAMPTZ DEFAULT NOW()
);

-- Create chips table
CREATE TABLE chips (
    id SERIAL PRIMARY KEY,
    nft_id INTEGER REFERENCES nfts(id),
    user_address VARCHAR(255),
    received BOOLEAN DEFAULT FALSE,
    created_at TIMESTAMPTZ DEFAULT NOW()
);

-- Insert some dummy data for testing
INSERT INTO nfts (received) VALUES (false), (false), (false), (false), (false);

-- Insert chips for the nfts
-- Assuming ids 1 to 5 are generated
INSERT INTO chips (nft_id, received) VALUES 
(1, false), (1, false),
(2, false), (2, false),
(3, false), (3, false),
(4, false), (4, false),
(5, false), (5, false);
