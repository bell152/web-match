use sqlx::FromRow;
use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};
use bigdecimal::BigDecimal;

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct SwapRequest {
    pub id: Option<i64>,
    pub user_address: String,
    pub zero_for_one: bool,
    pub amount_in_raw: String,
    pub amount_out_raw: String,
    pub token_decimals: i32,
    pub block_timestamp_raw: i64,
    pub timestamp_utc: DateTime<Utc>,
    pub created_at: Option<DateTime<Utc>>,
}

impl SwapRequest {
    pub fn new(
        user_address: String,
        zero_for_one: bool,
        amount_in_raw: String,
        amount_out_raw: String,
        block_timestamp_raw: i64,
        timestamp_utc: DateTime<Utc>,
    ) -> Self {
        Self {
            id: None,
            user_address,
            zero_for_one,
            amount_in_raw,
            amount_out_raw,
            token_decimals: 18,
            block_timestamp_raw,
            timestamp_utc,
            created_at: None,
        }
    }
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Kline {
    pub id: Option<i64>,
    pub pair_id: i64,
    pub interval: String,
    pub start_time: DateTime<Utc>,
    pub open_price: BigDecimal,
    pub high_price: BigDecimal,
    pub low_price: BigDecimal,
    pub close_price: BigDecimal,
    pub volume_base: BigDecimal,
    pub volume_quote: BigDecimal,
    pub updated_at: DateTime<Utc>,
}

// Internal Event Bus

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum AppEvent {
    Swap(SwapEvent),
    Airdrop(AirdropEvent),
    KlineUpdate(KlineUpdateEvent),
    UserMint(UserMintEvent),
    Transfer(TransferEvent),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SwapEvent {
    pub user: String,
    pub zero_for_one: bool,
    pub amount_in: String,
    pub amount_out: String,
    pub timestamp: u64,
    pub timestamp_str: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AirdropEvent {
    pub to: String,
    pub amount: String,
    pub timestamp: u64,
    pub timestamp_str: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KlineUpdateEvent {
    pub pair_id: i64,
    pub interval: String,
    pub start_time: i64, // Unix timestamp suitable for frontend
    pub open: String,
    pub high: String,
    pub low: String,
    pub close: String,
    pub volume_base: String,
    pub volume_quote: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserMintEvent {
    pub user: String,
    pub token_id: String,
    pub block_number: u64,
    pub remark: String,
    pub token_url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransferEvent {
    pub from: String,
    pub to: String,
    pub value: String,
    pub timestamp: u64,
    pub timestamp_str: String,
    pub block_number: u64,
    pub mint_remark: Option<String>,  // ✅ 新增：来自 HakuNFTMint 事件的 remark
}
