CREATE TABLE kline (
    id BIGSERIAL PRIMARY KEY,

    pair_id BIGINT NOT NULL,            -- 对应交易对，例如 STT/TokenB 的内部 ID
    interval VARCHAR(10) NOT NULL,      -- K 线周期（例如：1m, 5m, 1h, 1d）

    start_time TIMESTAMP NOT NULL,      -- 此 K 线的起始时间（对齐，比如 10:00:00）

    open_price NUMERIC(38, 18) NOT NULL,
    high_price NUMERIC(38, 18) NOT NULL,
    low_price  NUMERIC(38, 18) NOT NULL,
    close_price NUMERIC(38, 18) NOT NULL,

    volume_base NUMERIC(38, 18) NOT NULL DEFAULT 0, -- base token 成交量
    volume_quote NUMERIC(38, 18) NOT NULL DEFAULT 0, -- quote token 成交量

    updated_at TIMESTAMP NOT NULL DEFAULT NOW(),

    UNIQUE (pair_id, interval, start_time)
);

CREATE INDEX idx_kline_pair_interval_time ON kline (pair_id, interval, start_time);
