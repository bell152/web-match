use alloy::{
    providers::{Provider, ProviderBuilder, WsConnect},
    sol,
    rpc::types::Filter,
    primitives::Address,
    signers::local::PrivateKeySigner,
    network::EthereumWallet,
};
use tokio::sync::broadcast;
use tracing::{info, error, warn};
use axum::{
    Router,
    extract::{State, WebSocketUpgrade, ws::{Message, WebSocket}, Path},
    routing::{get, post},
    response::{Json, IntoResponse, Response},
    http::{StatusCode, header},
    body::Body,
    extract::Query,
};
use serde::{Serialize, Deserialize};
use moka::future::Cache;
use moka::Expiry;
use std::{sync::Arc, time::{Duration, Instant}};
use futures::stream::StreamExt;
use chrono::{DateTime, TimeZone, Utc};
use sqlx::PgPool;
use bigdecimal::BigDecimal;
use tokio::fs::File;
use tokio_util::io::ReaderStream;

use crate::services::service::root;
use crate::services::service::insert_swap_request;
use crate::services::service::update_kline;
use crate::entitys::entity::{AppEvent, SwapEvent, AirdropEvent, KlineUpdateEvent, UserMintEvent, TransferEvent};
// Define the Airdropped event using the sol! macro
sol! {
    #[derive(Debug)]
    event Airdropped(address indexed to, uint256 amount, uint256 timestamp);

    #[derive(Debug)]
    event SwapExecuted(
        address indexed user,
        bool zeroForOne,
        uint256 amountIn,
        uint256 amountOut,
        uint256 timestamp
    );

    #[derive(Debug)]
    event UserMint(
        uint256 indexed tokenId,
        address indexed user,
        string remark,
        string token_url
    );

    // ERC20 标准 Transfer 事件
    #[derive(Debug)]
    event Transfer(
        address indexed from,
        address indexed to,
        uint256 value
    );

    // ✅ 新增：UserTransfer 事件（来自 HakuToken 合约）
    #[derive(Debug)]
    event UserTransfer(
        address indexed from,
        address indexed to,
        uint256 value,
        uint256 timestamp,
        uint256 blockNumber,
        string remark
    );

    // ✅ 新增：HakuNFTMint 事件（来自 HukuNFT 合约）
    #[derive(Debug)]
    event HakuNFTMint(
        address indexed from,
        address indexed to,
        uint256 value,
        uint256 indexed tokenId,
        string remark
    );
}
    
pub const EXPIRE_LONG_TIME: u64 = 180000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Expiration {
    Never,
    AfterLongTime,
}

impl Expiration {
    pub fn as_duration(&self) -> Option<Duration> {
        match self {
            Expiration::Never => None,
            Expiration::AfterLongTime => Some(Duration::from_secs(EXPIRE_LONG_TIME)),
        }
    }
}

pub struct MyExpiry;
impl Expiry<String, (Expiration, (Vec<u8>, Vec<u8>))> for MyExpiry {
    fn expire_after_create(
        &self,
        _key: &String,
        value: &(Expiration, (Vec<u8>, Vec<u8>)),
        _current_time: Instant,
    ) -> Option<Duration> {
        let duration = value.0.as_duration();
        // info!("MyExpiry: expire_after_create called with key {_key} and value {value:?}. Returning {duration:?}.");
        duration
    }
}

pub fn get_app_cache() -> Cache<String, (Expiration, (Vec<u8>, Vec<u8>))> {
    let eviction_listener = |key: Arc<String>, _value: (Expiration, (Vec<u8>, Vec<u8>)), cause: moka::notification::RemovalCause| {
        info!("======== Evicted key {key}. Cause: {cause:?} =========");
    };
    let expiry = MyExpiry;
    Cache::builder()
        .max_capacity(1000)
        .expire_after(expiry)
        .eviction_listener(eviction_listener)
        .build()
}


// Query parameters for user swap lookup
#[derive(Debug, Deserialize)]
pub struct UserSwapQuery {
    pub user_address: String,
}

#[derive(Debug, Deserialize)]
pub struct KlineQuery {
    pub pair_id: Option<i64>,
    pub interval: Option<String>,
    pub limit: Option<i64>,
}

// Query parameters for user mint lookup
#[derive(Debug, Deserialize)]
pub struct UserMintQuery {
    pub user_address: String,
}

// Request body for user safe mint
#[derive(Debug, Deserialize)]
pub struct UserSafeMintRequest {
    pub user_address: String,
    pub nft_id: String,
}

// Response structure for user swap summary
#[derive(Debug, Serialize)]
pub struct UserSwapResponse {
    pub user_address: String,
    pub total_amount_in: String,
    pub total_amount_out: String,
    pub amount_difference: String,
    pub swap_records: Vec<SwapRequestRecord>,
}

// Response structure for user mint query
#[derive(Debug, Serialize)]
pub struct UserMintResponse {
    pub user_address: String,
    pub can_mint: i32, // 1: can mint, 0: cannot mint
    pub nfts: Vec<NftDetail>,
}

// Response structure for minted NFTs query
#[derive(Debug, Serialize)]
pub struct MintedNftItem {
    pub nft_id: i32,
    pub token_id: Option<String>,
    pub token_url: Option<String>,
    pub image_url: Option<String>,  // 新增：NFT的图片URL
}

#[derive(Debug, Serialize)]
pub struct MintedNftsResponse {
    pub total: i32,
    pub nfts: Vec<MintedNftItem>,
}

// Response structure for user safe mint
#[derive(Debug, Serialize)]
pub struct UserSafeMintResponse {
    pub success: bool,
    pub message: String,
    pub tx_hash: Option<String>,
    pub nft_id: String,
    pub user_address: String,
}

// Response structure for mint eligibility verification
#[derive(Debug, Serialize)]
pub struct MintEligibilityResponse {
    pub eligible: bool,
    pub message: String,
    pub contract_address: Option<String>,
    pub token_id: Option<String>,
    pub uint256_param: Option<u64>,
}

// Request for mint failed notification
#[derive(Debug, Deserialize)]
pub struct MintFailedRequest {
    pub user_address: String,
    pub nft_id: String,
    pub error: Option<String>,
}

// Simple response
#[derive(Debug, Serialize)]
pub struct SimpleResponse {
    pub success: bool,
    pub message: String,
}

// Query parameters for NFT user chips
#[derive(Debug, Deserialize)]
pub struct NftUserChipsQuery {
    pub nft_id: i32,
    pub user_address: String,
}

// Response structure for NFT user chips
#[derive(Debug, Serialize)]
pub struct NftUserChipsResponse {
    pub nft_id: i32,
    pub user_address: String,
    pub file_name: Option<String>,
    pub chips: Vec<ChipInfo>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ChipInfo {
    pub id: i32,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub w: Option<i32>,
    pub h: Option<i32>,
    pub file_name: Option<String>,
}

// Request body for batch chips API
#[derive(Debug, Deserialize)]
pub struct NftUserChipsBatchRequest {
    pub nft_id: i32,
    pub user_address: String,
}

// Response structure for batch chips API (includes base64 images)
#[derive(Debug, Serialize)]
pub struct NftUserChipsBatchResponse {
    pub nft_id: i32,
    pub user_address: String,
    pub file_name: Option<String>,
    pub chips: Vec<ChipInfoWithBase64>,
}

#[derive(Debug, Serialize)]
pub struct ChipInfoWithBase64 {
    pub id: i32,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub w: Option<i32>,
    pub h: Option<i32>,
    pub file_name: Option<String>,
    pub base64: Option<String>, // base64 encoded image data URI
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NftDetail {
    pub nft_id: i32,
    pub file_name: Option<String>,
    pub token_id: Option<String>,  // 新增：用于前端调用合约的 token_id
    pub all_chips_owned: bool,
    pub owned_chips_count: i64,
    pub total_chips_count: i64,
    pub is_mint: i32,  // 0: 未申请, 1: 申请中, 2: 已mint
}

// Request body for swap quote
#[derive(Debug, Deserialize)]
pub struct SwapQuoteRequest {
    pub amount_in: String,  // Amount as string (e.g., "1000000000000000000" for 1 ETH)
    pub zero_for_one: bool, // true if swapping currency0 -> currency1, false otherwise
}

// Response structure for swap quote
#[derive(Debug, Serialize)]
pub struct SwapQuoteResponse {
    pub success: bool,
    pub amount_out: Option<String>,  // Output amount as string
    pub error: Option<String>,        // Error message if failed
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct SwapRequestRecord {
    pub id: i64,
    pub user_address: String,
    pub zero_for_one: bool,
    pub amount_in_raw: BigDecimal,
    pub amount_out_raw: BigDecimal,
    pub token_decimals: i32,
    pub block_timestamp_raw: i64,
    pub timestamp_utc: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct AppStatus {
    pub cache: Cache<String, (Expiration, (Vec<u8>, Vec<u8>))>,
    pub tx: broadcast::Sender<AppEvent>,
    pub db_pool: PgPool,
}

pub async fn app_map() -> Router {
    // Load environment variables from .env file
    dotenv::dotenv().ok();
    
    // 1️⃣ Start WebSocket server
    let (tx, _rx) = broadcast::channel::<AppEvent>(100);

    // 2️⃣ Initialize database connection pool
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set in .env file");
    let db_pool = PgPool::connect(&database_url)
        .await
        .expect("Failed to connect to database");
    
    info!("Database connected successfully");

    // 3️⃣ Start Alloy WebSocket Provider (Listen for chain events)
    // 从配置加载合约地址
    let pool_config = crate::config::get_pool_config()
        .expect("Failed to load pool config");
    
    let ws_url = pool_config.ws_url.clone();
    let token_b_contract_address: Address = pool_config.token_b;
    let swap_contract_address: Address = pool_config.swap_executor;
    let nft_contract_address: Address = pool_config.nft_contract;
    
    
    let tx_clone = tx.clone();

    // Spawn the event listener task
    tokio::spawn(async move {
        if let Err(e) = listen_for_events(&ws_url, vec![token_b_contract_address, swap_contract_address, nft_contract_address], tx_clone).await {
            error!("Event listener failed: {:?}", e);
        }
    });

    // 4️⃣ Spawn database worker task
    let db_pool_clone = db_pool.clone();
    let tx_for_db = tx.clone();
    let cache_for_db = get_app_cache();
    tokio::spawn(async move {
        swap_requests_worker(db_pool_clone, tx_for_db, cache_for_db).await;
    });

    // 5️⃣ Spawn Kline worker task
    let db_pool_kline = db_pool.clone();
    let tx_for_kline = tx.clone();
    tokio::spawn(async move {
        kline_worker(db_pool_kline, tx_for_kline).await;
    });

    // 6️⃣ Spawn UserMint worker task
    let db_pool_mint = db_pool.clone();
    let tx_for_mint = tx.clone();
    tokio::spawn(async move {
        user_mint_worker(db_pool_mint, tx_for_mint).await;
    });

    // 7️⃣ Spawn Cache Invalidation worker task
    let cache_clone = get_app_cache();
    let tx_for_cache = tx.clone();
    tokio::spawn(async move {
        cache_invalidation_worker(cache_clone, tx_for_cache).await;
    });

    // 8️⃣ Spawn User Transfer worker task
    let db_pool_transfer = db_pool.clone();
    let tx_for_transfer = tx.clone();
    let cache_for_transfer = get_app_cache();
    tokio::spawn(async move {
        user_transfer_worker(db_pool_transfer, tx_for_transfer, cache_for_transfer).await;
    });

    // Shared state
    let shared_state: Arc<AppStatus> = Arc::new(AppStatus {
        cache: get_app_cache(),
        tx,
        db_pool,
    });

    Router::new()
        .route("/", post(root))
        .route("/ws", get(ws_handler))
        .route("/api/user-swaps", get(query_user_swaps))
        .route("/api/klines", get(query_klines))
        .route("/api/query-mint", get(query_mint))
        .route("/api/query-minted-nfts", get(query_minted_nfts))  // 查询所有已铸造的NFT
        .route("/api/verify-mint-eligibility", post(verify_mint_eligibility_api))
        .route("/api/mint-failed", post(mint_failed))
        .route("/api/user-safe-mint", post(user_safe_mint))  // 保留旧接口（后端代付模式）
        .route("/api/images/{file_name}", get(serve_image))
        .route("/api/tiles/{file_name}/{tile_name}", get(serve_tile))
        .route("/api/nft-user-chips", get(get_nft_user_chips))
        .route("/api/nft-user-chips-batch", post(get_nft_user_chips_batch))
        .with_state(shared_state)
}

// ✅ WebSocket Client Connection Handler
async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppStatus>>,
) -> impl axum::response::IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

// ✅ API Handler: Query User Swaps
async fn query_user_swaps(
    Query(params): Query<UserSwapQuery>,
    State(state): State<Arc<AppStatus>>,
) -> Json<UserSwapResponse> {
    let user_address = params.user_address.to_lowercase();
    info!("Querying user swaps for address: {}", user_address);
    // Query all records for the user
    let records = sqlx::query_as!(
        SwapRequestRecord,
        r#"
        SELECT 
            id, user_address, zero_for_one, 
            amount_in_raw, amount_out_raw, 
            token_decimals, block_timestamp_raw, 
            timestamp_utc, created_at
        FROM swap_requests
        WHERE LOWER(user_address) = $1
        ORDER BY created_at DESC
        "#,
        user_address
    )
    .fetch_all(&state.db_pool)
    .await
    .unwrap_or_else(|e| {
        error!("Failed to fetch user swaps: {:?}", e);
        vec![]
    });

    // Calculate totals
    let mut total_amount_in = BigDecimal::from(0);
    let mut total_amount_out = BigDecimal::from(0);

    for record in &records {
        total_amount_in += &record.amount_in_raw;
        total_amount_out += &record.amount_out_raw;
    }

    let amount_difference = &total_amount_in - &total_amount_out;

    Json(UserSwapResponse {
        user_address,
        total_amount_in: total_amount_in.to_string(),
        total_amount_out: total_amount_out.to_string(),
        amount_difference: amount_difference.to_string(),
        swap_records: records,
    })
}

// ✅ API Handler: Query Historical K-lines
async fn query_klines(
    Query(params): Query<KlineQuery>,
    State(state): State<Arc<AppStatus>>,
) -> Json<Vec<KlineUpdateEvent>> {
    let pair_id = params.pair_id.unwrap_or(1);
    let interval = params.interval.unwrap_or_else(|| "1m".to_string());
    let limit = params.limit.unwrap_or(100);

    info!("Querying klines for pair {}, interval {}, limit {}", pair_id, interval, limit);

    let records = sqlx::query!(
        r#"
        SELECT 
            pair_id, interval, start_time, 
            open_price, high_price, low_price, close_price, 
            volume_base, volume_quote
        FROM kline
        WHERE pair_id = $1 AND interval = $2
        ORDER BY start_time ASC
        LIMIT $3
        "#,
        pair_id,
        interval,
        limit
    )
    .fetch_all(&state.db_pool)
    .await
    .unwrap_or_else(|e| {
        error!("Failed to fetch klines: {:?}", e);
        vec![]
    });

    let events: Vec<KlineUpdateEvent> = records.into_iter().map(|rec| KlineUpdateEvent {
        pair_id: rec.pair_id,
        interval: rec.interval,
        start_time: rec.start_time.and_utc().timestamp(),
        open: rec.open_price.to_string(),
        high: rec.high_price.to_string(),
        low: rec.low_price.to_string(),
        close: rec.close_price.to_string(),
        volume_base: rec.volume_base.to_string(),
        volume_quote: rec.volume_quote.to_string(),
    }).collect();

    Json(events)
}

// ✅ API Handler: Query User Mint Eligibility (with Moka cache)
async fn query_mint(
    Query(params): Query<UserMintQuery>,
    State(state): State<Arc<AppStatus>>,
) -> Json<UserMintResponse> {
    let user_address = params.user_address.to_lowercase();
    info!("Querying mint eligibility for address: {}", user_address);

    // 🔥 Check if cache is enabled
    dotenv::dotenv().ok();
    let cache_enabled = std::env::var("CACHE_ENABLED")
        .unwrap_or_else(|_| "true".to_string())
        .parse::<bool>()
        .unwrap_or(true);

    // 🔥 Cache key for mint query
    let cache_key = format!("mint:{}", user_address);

    // 🔥 Try to get from cache first (if enabled)
    if cache_enabled {
        if let Some(cached_data) = state.cache.get(&cache_key).await {
            info!("Cache HIT for mint query: {}", user_address);
            let (_, (can_mint_bytes, nfts_bytes)) = cached_data;
            
            // Deserialize cached data
            if let (Ok(can_mint), Ok(nfts)) = (
                serde_json::from_slice::<i32>(&can_mint_bytes),
                serde_json::from_slice::<Vec<NftDetail>>(&nfts_bytes)
            ) {
                return Json(UserMintResponse {
                    user_address,
                    can_mint,
                    nfts,
                });
            } else {
                warn!("Failed to deserialize cached mint data for {}", user_address);
            }
        }
    } else {
        info!("Cache DISABLED - querying database directly");
    }

    info!("Cache MISS for mint query: {}", user_address);

    // Step 1: Query all NFTs owned by the user (received = true)
    let user_nfts = sqlx::query!(
        r#"
        SELECT id FROM nfts 
        WHERE LOWER(user_address) = $1 AND received = true
        "#,
        user_address
    )
    .fetch_all(&state.db_pool)
    .await
    .unwrap_or_else(|e| {
        error!("Failed to fetch user NFTs: {:?}", e);
        vec![]
    });

    if user_nfts.is_empty() {
        info!("User {} has no NFTs", user_address);
        
        // Cache the result (empty NFTs, can_mint = 0) if enabled
        let can_mint = 0;
        let nfts: Vec<NftDetail> = vec![];
        
        if cache_enabled {
            if let (Ok(can_mint_bytes), Ok(nfts_bytes)) = (
                serde_json::to_vec(&can_mint),
                serde_json::to_vec(&nfts)
            ) {
                state.cache.insert(
                    cache_key,
                    (Expiration::AfterLongTime, (can_mint_bytes, nfts_bytes))
                ).await;
            }
        }
        
        return Json(UserMintResponse {
            user_address,
            can_mint,
            nfts,
        });
    }

    let mut nft_details = Vec::new();
    let mut has_any_complete_nft = false;

    // Step 2: For each NFT, check if all chips are owned by the user
    for nft_record in user_nfts {
        let nft_id = nft_record.id;

        // Query NFT details including file_name, token_id and is_mint
        let nft_info = sqlx::query!(
            r#"
            SELECT file_name, token_id, is_mint
            FROM nfts
            WHERE id = $1
            "#,
            nft_id
        )
        .fetch_one(&state.db_pool)
        .await;

        let (file_name, token_id, is_mint) = match nft_info {
            Ok(record) => (record.file_name, record.token_id, record.is_mint),
            Err(e) => {
                error!("Failed to fetch NFT info for NFT {}: {:?}", nft_id, e);
                (None, None, 0)
            }
        };

        // Count total chips for this NFT
        let total_chips_result = sqlx::query!(
            "SELECT COUNT(*) as count FROM chips WHERE nft_id = $1",
            nft_id
        )
        .fetch_one(&state.db_pool)
        .await;

        let total_count = match total_chips_result {
            Ok(record) => record.count.unwrap_or(0),
            Err(e) => {
                error!("Failed to count total chips for NFT {}: {:?}", nft_id, e);
                0
            }
        };

        // Count chips owned by the user for this NFT
        let owned_chips_result = sqlx::query!(
            r#"
            SELECT COUNT(*) as count FROM chips 
            WHERE nft_id = $1 AND LOWER(user_address) = $2 AND received = true
            "#,
            nft_id,
            user_address
        )
        .fetch_one(&state.db_pool)
        .await;

        let owned_count = match owned_chips_result {
            Ok(record) => record.count.unwrap_or(0),
            Err(e) => {
                error!("Failed to count owned chips for NFT {}: {:?}", nft_id, e);
                0
            }
        };

        let all_chips_owned = total_count > 0 && owned_count == total_count;

        // 将数据库中的 token_id (i64) 转换为字符串
        let token_id_str = token_id.map(|id| id.to_string());

        nft_details.push(NftDetail {
            nft_id,
            file_name,
            token_id: token_id_str,  // 使用转换后的字符串
            all_chips_owned,
            owned_chips_count: owned_count,
            total_chips_count: total_count,
            is_mint,
        });

        // If any NFT has all chips, user can mint
        if all_chips_owned {
            has_any_complete_nft = true;
        }
    }

    // User can mint if they have at least one complete NFT
    let can_mint = if has_any_complete_nft { 1 } else { 0 };

    info!(
        "User {} mint eligibility: {} (NFTs: {}, Has complete NFT: {})",
        user_address,
        can_mint,
        nft_details.len(),
        has_any_complete_nft
    );

    // 🔥 Store result in cache (if enabled)
    if cache_enabled {
        if let (Ok(can_mint_bytes), Ok(nfts_bytes)) = (
            serde_json::to_vec(&can_mint),
            serde_json::to_vec(&nft_details)
        ) {
            state.cache.insert(
                cache_key,
                (Expiration::AfterLongTime, (can_mint_bytes, nfts_bytes))
            ).await;
            info!("Cached mint query result for {}", user_address);
        }
    } else {
        info!("Cache DISABLED - skipping cache write");
    }

    Json(UserMintResponse {
        user_address: user_address.clone(),
        can_mint,
        nfts: nft_details,
    })
}

// 辅助函数：从本地 IPFS 节点获取 NFT 元数据并提取 image URL
async fn fetch_nft_image_url(token_url: &str) -> Option<String> {
    // 从环境变量读取 IPFS Metadata CID（必须配置，否则报错）
    let ipfs_metadata_cid = match std::env::var("IPFS_METADATA_CID") {
        Ok(cid) => cid,
        Err(_) => {
            error!("❌ IPFS_METADATA_CID not set in .env file! Please configure it.");
            return None;
        }
    };
    
    let ipfs_metadata_path = format!("{}/{}.json", ipfs_metadata_cid, token_url);
    
    // 使用本地 IPFS 节点执行 ipfs cat 命令
    match tokio::process::Command::new("ipfs")
        .arg("cat")
        .arg(&ipfs_metadata_path)
        .output()
        .await
    {
        Ok(output) => {
            if output.status.success() {
                // 解析 JSON 输出
                match serde_json::from_slice::<serde_json::Value>(&output.stdout) {
                    Ok(json) => {
                        // 提取 image 字段
                        if let Some(image) = json.get("image").and_then(|v| v.as_str()) {
                            info!("✅ Fetched image URL from local IPFS for {}: {}", token_url, image);
                            return Some(image.to_string());
                        } else {
                            warn!("⚠️ No 'image' field found in metadata for token_url: {}", token_url);
                        }
                    }
                    Err(e) => {
                        error!("❌ Failed to parse JSON from IPFS path {}: {:?}", ipfs_metadata_path, e);
                    }
                }
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                warn!("⚠️ IPFS cat failed for {}: {}", ipfs_metadata_path, stderr);
            }
        }
        Err(e) => {
            error!("❌ Failed to execute ipfs cat for {}: {:?}", ipfs_metadata_path, e);
        }
    }
    None
}

// ✅ API Handler: Query All Minted NFTs
// Returns the latest 10 NFTs that have been successfully minted (is_mint = 2, received = true)
async fn query_minted_nfts(
    State(state): State<Arc<AppStatus>>,
) -> Json<MintedNftsResponse> {
    info!("Querying latest 10 minted NFTs");

    // Query the latest 10 NFTs with is_mint = 2 and received = true, ordered by updated_at DESC
    let minted_nfts = sqlx::query!(
        r#"
        SELECT id, token_id, token_url
        FROM nfts
        WHERE is_mint = 2 AND received = true
        ORDER BY updated_at DESC
        LIMIT 10
        "#
    )
    .fetch_all(&state.db_pool)
    .await
    .unwrap_or_else(|e| {
        error!("Failed to fetch minted NFTs: {:?}", e);
        vec![]
    });

    let total = minted_nfts.len() as i32;
    
    // 并发获取所有 NFT 的图片 URL
    let mut nft_items = Vec::new();
    for record in minted_nfts {
        let token_id_str = record.token_id.map(|id| id.to_string());
        let token_url = record.token_url.clone();
        
        // 如果有 token_url，尝试获取 image_url
        let image_url = if let Some(ref url) = token_url {
            fetch_nft_image_url(url).await
        } else {
            None
        };
        
        nft_items.push(MintedNftItem {
            nft_id: record.id,
            token_id: token_id_str,
            token_url,
            image_url,
        });
    }

    info!("Found {} minted NFTs (limited to 10) with image URLs", total);

    Json(MintedNftsResponse {
        total,
        nfts: nft_items,
    })
}

// ✅ API Handler: User Safe Mint
async fn user_safe_mint(
    State(state): State<Arc<AppStatus>>,
    axum::extract::Json(request): axum::extract::Json<UserSafeMintRequest>,
) -> Json<UserSafeMintResponse> {
    let user_address = request.user_address.to_lowercase();
    let nft_id = request.nft_id.clone();
    
    info!("Processing safe mint for user: {}, nft_id: {}", user_address, nft_id);

    // 🔒 Step 1: Verify NFT ownership and chips completeness
    match verify_nft_mint_eligibility(&state.db_pool, &user_address, &nft_id).await {
        Ok(false) => {
            warn!("User {} is not eligible to mint nft_id: {}", user_address, nft_id);
            
            // Check specific reason for better error message
            let nft_id_num: i32 = nft_id.parse().unwrap_or(0);
            let is_mint_status = sqlx::query!(
                r#"SELECT is_mint FROM nfts WHERE id = $1"#,
                nft_id_num
            )
            .fetch_optional(&state.db_pool)
            .await
            .ok()
            .and_then(|r| r)
            .map(|r| r.is_mint)
            .unwrap_or(0);
            
            let message = match is_mint_status {
                1 => format!("NFT {} is already being minted, please wait", nft_id),
                2 => format!("NFT {} has already been minted", nft_id),
                _ => format!(
                    "Cannot mint: NFT {} either doesn't belong to you or its chips are incomplete",
                    nft_id
                ),
            };
            
            return Json(UserSafeMintResponse {
                success: false,
                message,
                tx_hash: None,
                nft_id,
                user_address,
            });
        }
        Err(e) => {
            error!("Failed to verify mint eligibility: {:?}", e);
            return Json(UserSafeMintResponse {
                success: false,
                message: format!("Failed to verify mint eligibility: {}", e),
                tx_hash: None,
                nft_id,
                user_address,
            });
        }
        Ok(true) => {
            info!("✅ User {} is eligible to mint nft_id: {}", user_address, nft_id);
        }
    }

    // Load private key from environment
    dotenv::dotenv().ok();
    let private_key = match std::env::var("PRIVATE_KEY") {
        Ok(key) => key,
        Err(_) => {
            error!("PRIVATE_KEY not found in environment");
            return Json(UserSafeMintResponse {
                success: false,
                message: "Server configuration error: PRIVATE_KEY not set".to_string(),
                tx_hash: None,
                nft_id,
                user_address,
            });
        }
    };

    // Load NFT contract address from configuration
    let pool_config = match crate::config::get_pool_config() {
        Ok(config) => config,
        Err(e) => {
            error!("Failed to load pool config: {}", e);
            return Json(UserSafeMintResponse {
                success: false,
                message: format!("Configuration error: {}", e),
                tx_hash: None,
                nft_id,
                user_address,
            });
        }
    };
    let contract_address = pool_config.nft_contract;

    // Parse user address
    let to_address: Address = match user_address.parse() {
        Ok(addr) => addr,
        Err(e) => {
            error!("Failed to parse user address: {:?}", e);
            return Json(UserSafeMintResponse {
                success: false,
                message: format!("Invalid user address: {}", e),
                tx_hash: None,
                nft_id,
                user_address,
            });
        }
    };

    // 🔄 Step 1: Query file_name and extract number for uint256 parameter
    info!("Step 1: Querying NFT file_name for uint256 parameter");
    let nft_id_num: i32 = match nft_id.parse() {
        Ok(id) => id,
        Err(e) => {
            error!("Failed to parse nft_id: {:?}", e);
            return Json(UserSafeMintResponse {
                success: false,
                message: format!("Invalid nft_id: {}", e),
                tx_hash: None,
                nft_id,
                user_address,
            });
        }
    };

    let uint256_param = match sqlx::query!(
        r#"SELECT file_name FROM nfts WHERE id = $1"#,
        nft_id_num
    )
    .fetch_one(&state.db_pool)
    .await
    {
        Ok(record) => {
            let file_name = record.file_name.unwrap_or_else(|| nft_id.clone());
            // Extract number from file_name (e.g., "27.png" -> 27)
            let uint_value = std::path::Path::new(&file_name)
                .file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(nft_id_num as u64);
            info!("✅ Uint256 parameter prepared: {} (from file_name: {})", uint_value, file_name);
            uint_value
        }
        Err(e) => {
            error!("Failed to query file_name: {:?}", e);
            // Use nft_id as fallback
            nft_id_num as u64
        }
    };

    // 🔄 Step 2: Update database first - set is_mint = 1 (申请中)
    info!("Step 2: Updating database status to 'applying' (is_mint=1)");
    match update_nft_mint_status(&state.db_pool, &user_address, &nft_id, 1).await {
        Ok(_) => {
            info!("✅ Updated NFT is_mint status to 1 for nft_id: {}", nft_id);
        }
        Err(e) => {
            error!("Failed to update NFT mint status: {:?}", e);
            return Json(UserSafeMintResponse {
                success: false,
                message: format!("Failed to update database: {}", e),
                tx_hash: None,
                nft_id,
                user_address,
            });
        }
    }

    // 🔄 Step 3: Call contract safeMint function
    info!("Step 3: Calling smart contract safeMint");
    match call_safe_mint_contract(contract_address, to_address, nft_id.clone(), uint256_param, private_key).await {
        Ok(tx_hash) => {
            info!("✅ SafeMint transaction sent: {}", tx_hash);
            
            // Invalidate mint cache after successful transaction
            let cache_key = format!("mint:{}", user_address);
            state.cache.invalidate(&cache_key).await;
            info!("🗑️  Invalidated mint cache for user: {}", user_address);
            
            Json(UserSafeMintResponse {
                success: true,
                message: "Mint transaction submitted successfully".to_string(),
                tx_hash: Some(tx_hash),
                nft_id,
                user_address,
            })
        }
        Err(e) => {
            error!("❌ Failed to call safeMint: {:?}", e);
            
            // Rollback database status if contract call fails
            warn!("Attempting to rollback database status due to contract call failure");
            if let Err(rollback_err) = update_nft_mint_status(&state.db_pool, &user_address, &nft_id, 0).await {
                error!("Failed to rollback NFT mint status: {:?}", rollback_err);
            } else {
                info!("✅ Rolled back NFT is_mint status to 0");
            }
            
            Json(UserSafeMintResponse {
                success: false,
                message: format!("Failed to mint: {}", e),
                tx_hash: None,
                nft_id,
                user_address,
            })
        }
    }
}

// ✅ API Handler: Verify Mint Eligibility (User Self-Pay Mode)
// This endpoint verifies eligibility and returns contract parameters for frontend to call
async fn verify_mint_eligibility_api(
    State(state): State<Arc<AppStatus>>,
    axum::extract::Json(request): axum::extract::Json<UserSafeMintRequest>,
) -> Json<MintEligibilityResponse> {
    let user_address = request.user_address.to_lowercase();
    let nft_id = request.nft_id.clone();
    
    info!("Verifying mint eligibility for user: {}, nft_id: {}", user_address, nft_id);

    // Step 1: Verify NFT ownership and chips completeness
    match verify_nft_mint_eligibility(&state.db_pool, &user_address, &nft_id).await {
        Ok(false) => {
            warn!("User {} is not eligible to mint nft_id: {}", user_address, nft_id);
            
            // Check specific reason for better error message
            let nft_id_num: i32 = nft_id.parse().unwrap_or(0);
            let is_mint_status = sqlx::query!(
                r#"SELECT is_mint FROM nfts WHERE id = $1"#,
                nft_id_num
            )
            .fetch_optional(&state.db_pool)
            .await
            .ok()
            .and_then(|r| r)
            .map(|r| r.is_mint)
            .unwrap_or(0);
            
            let message = match is_mint_status {
                1 => format!("NFT {} is already being minted, please wait", nft_id),
                2 => format!("NFT {} has already been minted", nft_id),
                _ => format!(
                    "Cannot mint: NFT {} either doesn't belong to you or its chips are incomplete",
                    nft_id
                ),
            };
            
            return Json(MintEligibilityResponse {
                eligible: false,
                message,
                contract_address: None,
                token_id: None,
                uint256_param: None,
            });
        }
        Err(e) => {
            error!("Failed to verify mint eligibility: {:?}", e);
            return Json(MintEligibilityResponse {
                eligible: false,
                message: format!("Failed to verify eligibility: {}", e),
                contract_address: None,
                token_id: None,
                uint256_param: None,
            });
        }
        Ok(true) => {
            info!("✅ User {} is eligible to mint nft_id: {}", user_address, nft_id);
        }
    }

    // Step 2: Extract uint256 parameter from file_name
    let nft_id_num: i32 = match nft_id.parse() {
        Ok(id) => id,
        Err(e) => {
            error!("Failed to parse nft_id: {:?}", e);
            return Json(MintEligibilityResponse {
                eligible: false,
                message: format!("Invalid nft_id: {}", e),
                contract_address: None,
                token_id: None,
                uint256_param: None,
            });
        }
    };

    let uint256_param = match sqlx::query!(
        r#"SELECT file_name FROM nfts WHERE id = $1"#,
        nft_id_num
    )
    .fetch_one(&state.db_pool)
    .await
    {
        Ok(record) => {
            let file_name = record.file_name.unwrap_or_else(|| nft_id.clone());
            let uint_value = std::path::Path::new(&file_name)
                .file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(nft_id_num as u64);
            info!("✅ Uint256 parameter: {} (from file_name: {})", uint_value, file_name);
            uint_value
        }
        Err(e) => {
            error!("Failed to query file_name: {:?}", e);
            nft_id_num as u64
        }
    };

    // Step 3: Update database status to "applying" (is_mint=1)
    info!("Updating NFT status to 'applying' (is_mint=1)");
    match update_nft_mint_status(&state.db_pool, &user_address, &nft_id, 1).await {
        Ok(_) => {
            info!("✅ Updated NFT is_mint status to 1 for nft_id: {}", nft_id);
        }
        Err(e) => {
            error!("Failed to update NFT mint status: {:?}", e);
            return Json(MintEligibilityResponse {
                eligible: false,
                message: format!("Failed to update database: {}", e),
                contract_address: None,
                token_id: None,
                uint256_param: None,
            });
        }
    }

    // Step 4: Load NFT contract address from configuration and return contract parameters
    let pool_config = match crate::config::get_pool_config() {
        Ok(config) => config,
        Err(e) => {
            error!("Failed to load pool config: {}", e);
            return Json(MintEligibilityResponse {
                eligible: false,
                message: format!("Configuration error: {}", e),
                contract_address: None,
                token_id: None,
                uint256_param: None,
            });
        }
    };
    
    Json(MintEligibilityResponse {
        eligible: true,
        message: "You can proceed with minting. Use your wallet to call the contract.".to_string(),
        contract_address: Some(format!("{}", pool_config.nft_contract)),
        token_id: Some(nft_id),
        uint256_param: Some(uint256_param),
    })
}

// ✅ API Handler: Mint Failed Notification
// Called by frontend when user cancels or transaction fails
async fn mint_failed(
    State(state): State<Arc<AppStatus>>,
    axum::extract::Json(request): axum::extract::Json<MintFailedRequest>,
) -> Json<SimpleResponse> {
    let user_address = request.user_address.to_lowercase();
    let nft_id = request.nft_id;
    let error_msg = request.error.unwrap_or_else(|| "User cancelled or transaction failed".to_string());
    
    warn!("Mint failed notification: user={}, nft_id={}, error={}", user_address, nft_id, error_msg);
    
    // Rollback status to is_mint=0
    match update_nft_mint_status(&state.db_pool, &user_address, &nft_id, 0).await {
        Ok(_) => {
            info!("✅ Rolled back is_mint to 0 for failed mint: nft_id={}", nft_id);
            
            // Invalidate cache
            let cache_key = format!("mint:{}", user_address);
            state.cache.invalidate(&cache_key).await;
            info!("🗑️  Invalidated mint cache for user: {}", user_address);
            
            Json(SimpleResponse {
                success: true,
                message: "Status rolled back successfully".to_string(),
            })
        }
        Err(e) => {
            error!("Failed to rollback status: {:?}", e);
            Json(SimpleResponse {
                success: false,
                message: format!("Failed to rollback: {}", e),
            })
        }
    }
}

async fn handle_socket(mut socket: WebSocket, state: Arc<AppStatus>) {
    let mut rx = state.tx.subscribe();
    while let Ok(msg) = rx.recv().await {
        // Serialize message to JSON
        let json_msg = match serde_json::to_string(&msg) {
            Ok(json) => json,
            Err(e) => {
                error!("Failed to serialize message: {:?}", e);
                continue;
            }
        };

        if socket.send(Message::Text(json_msg.into())).await.is_err() {
            info!("Client disconnected");
            break;
        }
    }
}

/// Establish WebSocket connection and listen for chain events
async fn listen_for_events(
    ws_url: &str,
    contract_addresses: Vec<Address>,
    tx: broadcast::Sender<AppEvent>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    info!("Attempting to connect to WebSocket: {}", ws_url);

    // Establish WebSocket connection
    let ws = WsConnect::new(ws_url);
    let provider = ProviderBuilder::new()
        .connect_ws(ws)
        .await
        .map_err(|e| format!("Failed to connect to WebSocket: {:?}", e))?;

    info!("Successfully connected to WebSocket");

    // ✅ 保存 RPC URL 用于在异步任务中创建 HTTP provider
    dotenv::dotenv().ok();
    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| ws_url.replace("wss://", "https://").replace("ws://", "http://"));
    let rpc_url_clone = rpc_url.clone();

    // Create filter for the contract addresses
    let filter = Filter::new()
        .address(contract_addresses);

    // Subscribe to logs
    let sub = provider.subscribe_logs(&filter).await?;
    let mut stream = sub.into_stream();

    info!("Listening for Airdropped and SwapExecuted events...");

    while let Some(log) = stream.next().await {
        // Try to decode Airdropped
        if let Ok(decoded) = log.log_decode::<Airdropped>() {
            let event = decoded.inner;
            info!("🎉 New Airdrop Event!");
            info!("To: {:?}", event.to);
            
            // Format timestamp
            let timestamp_val = event.timestamp.saturating_to::<u64>();
            let dt = Utc.timestamp_opt(timestamp_val as i64, 0).unwrap();
            let formatted_time = dt.format("%Y-%m-%d %H:%M:%S UTC").to_string();

            info!("Amount: {}", event.amount);
            info!("timestamp: {} ({})", event.timestamp, formatted_time);

            let app_event = AppEvent::Airdrop(AirdropEvent {
                to: event.to.to_string(),
                amount: event.amount.to_string(),
                timestamp: timestamp_val,
                timestamp_str: formatted_time,
            });

            // Send message to all connected WebSocket clients
            if let Err(_e) = tx.send(app_event) {
                info!("No clients connected, skipping broadcast");
            }
        } 
        // Try to decode SwapExecuted
        else if let Ok(decoded) = log.log_decode::<SwapExecuted>() {
            let event = decoded.inner;
            info!("🔄 New Swap Event!");
            info!("User: {:?}", event.user);
            info!("ZeroForOne: {}", event.zeroForOne);  
            // Format timestamp
            let timestamp_val = event.timestamp.saturating_to::<u64>();
            let dt = Utc.timestamp_opt(timestamp_val as i64, 0).unwrap();
            let formatted_time = dt.format("%Y-%m-%d %H:%M:%S UTC").to_string();
            
            let amount_in_readable = event.amountIn.to_string().parse::<f64>()
                .map(|v| v / 1e18).unwrap_or(0.0);
            let amount_out_readable = event.amountOut.to_string().parse::<f64>()
                .map(|v| v / 1e18).unwrap_or(0.0);
            let price = if amount_in_readable > 0.0 {
                amount_out_readable / amount_in_readable
            } else { 0.0 };
            
            info!("AmountIn: {} ({:.6} tokens)", event.amountIn, amount_in_readable);
            info!("AmountOut: {} ({:.6} tokens)", event.amountOut, amount_out_readable);
            info!("Price: {:.6} (1 TokenIn = {:.6} TokenOut)", price, price);
            info!("Timestamp: {} ({})", event.timestamp, formatted_time);

            let app_event = AppEvent::Swap(SwapEvent {
                user: event.user.to_string(),
                zero_for_one: event.zeroForOne,
                amount_in: event.amountIn.to_string(),
                amount_out: event.amountOut.to_string(),
                timestamp: timestamp_val,
                timestamp_str: formatted_time,
            });

            if let Err(_e) = tx.send(app_event) {
                info!("No clients connected, skipping broadcast");
            }
        }
        // Try to decode UserMint
        else if let Ok(decoded) = log.log_decode::<UserMint>() {
            let event = decoded.inner;
            let block_num = log.block_number.unwrap_or(0);
            
            info!("🎨 New UserMint Event!");
            info!("User: {:?}", event.user);
            info!("TokenId: {}", event.tokenId);
            info!("blockNumber: {}", block_num);
            info!("Token URL: {}", event.token_url);

            let app_event = AppEvent::UserMint(UserMintEvent {
                user: event.user.to_string(),
                token_id: event.tokenId.to_string(),
                block_number: block_num,
                remark: event.remark.to_string(),
                token_url: event.token_url.to_string(),
            });

            if let Err(_e) = tx.send(app_event) {
                info!("No clients connected, skipping broadcast");
            }
        }
        // ✅ 监听 UserTransfer 事件（来自 HakuToken 合约）
        else if let Ok(decoded) = log.log_decode::<UserTransfer>() {
            let event = decoded.inner;
            let block_num = log.block_number.unwrap_or(0);
            let _block_timestamp = log.block_timestamp.unwrap_or(0);
            
            // 获取交易哈希
            let tx_hash = match log.transaction_hash {
                Some(hash) => hash,
                None => {
                    warn!("UserTransfer event has no transaction hash, skipping");
                    continue;
                }
            };
            
            info!("💸 New UserTransfer Event!");
            info!("From: {:?}", event.from);
            info!("To: {:?}", event.to);
            info!("Value: {}", event.value);
            info!("Block: {}", block_num);
            info!("Transaction Hash: {:?}", tx_hash);
            
            // ✅ 异步获取交易收据并解析 HakuNFTMint 事件
            let rpc_url_for_task = rpc_url_clone.clone();
            let tx_sender = tx.clone();
            
            tokio::spawn(async move {
                // 在异步任务中创建 HTTP provider
                let http_provider = match rpc_url_for_task.parse() {
                    Ok(url) => ProviderBuilder::new().connect_http(url),
                    Err(e) => {
                        error!("Failed to parse RPC URL: {:?}", e);
                        return;
                    }
                };
                
                // 获取交易收据
                let receipt = match http_provider.get_transaction_receipt(tx_hash).await {
                    Ok(Some(r)) => r,
                    Ok(None) => {
                        // 如果收据不存在，简单重试一次（处理节点同步延迟）
                        warn!("Transaction receipt not found, retrying once...");
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        match http_provider.get_transaction_receipt(tx_hash).await {
                            Ok(Some(r)) => r,
                            Ok(None) => {
                                error!("Transaction receipt not found after retry for tx: {:?}", tx_hash);
                                return;
                            }
                            Err(e) => {
                                error!("Failed to get transaction receipt: {:?}", e);
                                return;
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to get transaction receipt: {:?}", e);
                        return;
                    }
                };
                
                // ✅ 从交易收据中查找 HakuNFTMint 事件
                let mut mint_remark: Option<String> = None;
                
                // 获取日志（TransactionReceipt 的 logs 字段）
                for receipt_log in receipt.logs() {
                    if let Ok(decoded_mint) = receipt_log.log_decode::<HakuNFTMint>() {
                        let mint_event = decoded_mint.inner;
                        info!("🎨 Found HakuNFTMint event in transaction receipt!");
                        info!("  From: {:?}", mint_event.from);
                        info!("  To: {:?}", mint_event.to);
                        info!("  TokenId: {}", mint_event.tokenId);
                        info!("  Remark: {}", mint_event.remark);
                        
                        mint_remark = Some(mint_event.remark.to_string());
                        break;  // 通常一个交易只有一个 HakuNFTMint
                    }
                }
                
                if mint_remark.is_none() {
                    info!("ℹ️  No HakuNFTMint event found in this transaction (normal user transfer)");
                }
                
                // 格式化时间戳
                let timestamp_val = event.timestamp.saturating_to::<u64>();
                let formatted_time = chrono::Utc.timestamp_opt(timestamp_val as i64, 0)
                    .unwrap()
                    .format("%Y-%m-%d %H:%M:%S UTC")
                    .to_string();
                
                let value_readable = event.value.to_string().parse::<f64>()
                    .map(|v| v / 1e18)
                    .unwrap_or(0.0);
                
                info!("💸 Processing UserTransfer: {} -> {}, value: {} ({:.6} tokens), mint_remark: {:?}", 
                    event.from, event.to, event.value, value_readable, mint_remark);
                
                // ✅ 创建 TransferEvent，包含 mint_remark
                let app_event = AppEvent::Transfer(TransferEvent {
                    from: event.from.to_string(),
                    to: event.to.to_string(),
                    value: event.value.to_string(),
                    timestamp: timestamp_val,
                    timestamp_str: formatted_time,
                    block_number: event.blockNumber.saturating_to::<u64>(),
                    mint_remark,  // ✅ 传递 mint_remark
                });
                
                if let Err(_e) = tx_sender.send(app_event) {
                    info!("No clients connected, skipping broadcast");
                }
            });
        }
    }

    Ok(())
}

/// Database worker that subscribes to broadcast channel and inserts events into database
async fn swap_requests_worker(db_pool: PgPool, tx: broadcast::Sender<AppEvent>, cache: Cache<String, (Expiration, (Vec<u8>, Vec<u8>))>) {
    let mut rx = tx.subscribe();
    info!("Database worker started, listening for events...");

    while let Ok(msg) = rx.recv().await {
        if let AppEvent::Swap(swap_event) = msg {
            let user_address = swap_event.user.clone();
            let zero_for_one = swap_event.zero_for_one;
            let amount_in = swap_event.amount_in.clone();
            let amount_out = swap_event.amount_out.clone();
            let timestamp_raw = swap_event.timestamp as i64;
            let timestamp_utc = Utc.timestamp_opt(timestamp_raw, 0).unwrap();

            let data = (user_address.clone(), zero_for_one, amount_in.clone(), amount_out.clone(), timestamp_raw, timestamp_utc);

            match insert_swap_request(&db_pool, data).await {
                Ok(id) => {
                    info!("✅ Inserted swap request with ID: {}", id);
                }
                Err(e) => {
                    error!("❌ Failed to insert swap request: {:?}", e);
                }
            }
        }
    }
}

/// Kline worker that subscribes to broadcast channel and updates kline data
async fn kline_worker(db_pool: PgPool, tx: broadcast::Sender<AppEvent>) {
    let mut rx = tx.subscribe();
    info!("Kline worker started, listening for events...");

    while let Ok(msg) = rx.recv().await {
        if let AppEvent::Swap(swap_event) = msg {
            let user_address = swap_event.user.clone();
            let zero_for_one = swap_event.zero_for_one;
            let amount_in = swap_event.amount_in.clone();
            let amount_out = swap_event.amount_out.clone();
            let timestamp_raw = swap_event.timestamp as i64;
            let timestamp_utc = Utc.timestamp_opt(timestamp_raw, 0).unwrap();

            let data = (user_address, zero_for_one, amount_in, amount_out, timestamp_raw, timestamp_utc);

            match update_kline(&db_pool, data).await {
                Ok(events) => {
                    for event in events {
                        if let Err(e) = tx.send(AppEvent::KlineUpdate(event)) {
                             error!("Failed to broadcast KlineUpdate: {:?}", e);
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to update kline: {:?}", e);
                }
            }
        }
    }
}

/// UserMint worker that subscribes to broadcast channel and processes UserMint events
async fn user_mint_worker(db_pool: PgPool, tx: broadcast::Sender<AppEvent>) {
    let mut rx = tx.subscribe();
    info!("UserMint worker started, listening for events...");

    while let Ok(msg) = rx.recv().await {
        if let AppEvent::UserMint(mint_event) = msg {
            info!("🎨 Received UserMint event:");
            info!("  User: {}", mint_event.user);
            info!("  TokenId: {}", mint_event.token_id);
            info!("  BlockNumber: {}", mint_event.block_number);
            info!("  Remark (NFT_ID): {}", mint_event.remark);
            info!("  Token URL: {}", mint_event.token_url);
            
            // Process the mint event
            match crate::services::service::process_user_mint_event(
                &db_pool,
                &mint_event.user,
                &mint_event.token_id,
                mint_event.block_number,
                &mint_event.remark,
                &mint_event.token_url,
            ).await {
                Ok(_) => {
                    info!("✅ Successfully processed UserMint event for user: {}", mint_event.user);
                }
                Err(e) => {
                    error!("❌ Failed to process UserMint event: {:?}", e);
                }
            }
        }
    }
}

/// User Transfer Worker - 处理 Transfer 事件（用户转账）
/// 
/// 监听 Transfer 事件，处理用户之间的 token 转账：
/// - from 地址：执行 revert_chips（转出余额）
/// - to 地址：执行 receive_chips（增加余额）
async fn user_transfer_worker(
    db_pool: PgPool, 
    tx: broadcast::Sender<AppEvent>,
    cache: Cache<String, (Expiration, (Vec<u8>, Vec<u8>))>
) {
    let mut rx = tx.subscribe();
    info!("💸 User Transfer worker started, listening for Transfer events...");

    while let Ok(msg) = rx.recv().await {
        if let AppEvent::Transfer(transfer_event) = msg {
            let from_address = transfer_event.from.to_lowercase();
            let to_address = transfer_event.to.to_lowercase();
            
            info!("💸 Received Transfer event:");
            info!("  From: {}", from_address);
            info!("  To: {}", to_address);
            info!("  Value: {}", transfer_event.value);
            info!("  Block: {}", transfer_event.block_number);
            info!("  Timestamp: {}", transfer_event.timestamp_str);
            if let Some(ref remark) = transfer_event.mint_remark {
                info!("  Mint Remark: {}", remark);
            } else {
                info!("  Mint Remark: None (normal user transfer)");
            }
            
            // 调用 service 中的 process_transfer_event
            match crate::services::service::process_transfer_event(
                &db_pool,
                &from_address,
                &to_address,
                &transfer_event.value,
                transfer_event.mint_remark.as_deref(),  // ✅ 传递 mint_remark
            ).await {
                Ok(_) => {
                    info!("✅ Successfully processed Transfer event: {} -> {}", 
                        from_address, to_address);
                    
                    // 🔥 清除 from 用户的缓存（转出方）
                    let from_cache_key = format!("mint:{}", from_address);
                    cache.invalidate(&from_cache_key).await;
                    info!("🗑️  Invalidated cache for sender: {}", from_address);
                    
                    // 🔥 清除 to 用户的缓存（接收方）
                    let to_cache_key = format!("mint:{}", to_address);
                    cache.invalidate(&to_cache_key).await;
                    info!("🗑️  Invalidated cache for receiver: {}", to_address);
                }
                Err(e) => {
                    error!("❌ Failed to process Transfer event: {:?}", e);
                }
            }
        }
    }
}

/// Cache invalidation worker that clears mint query cache when data changes
async fn cache_invalidation_worker(
    cache: Cache<String, (Expiration, (Vec<u8>, Vec<u8>))>,
    tx: broadcast::Sender<AppEvent>
) {
    let mut rx = tx.subscribe();
    info!("Cache invalidation worker started, listening for events...");

    while let Ok(msg) = rx.recv().await {
        match msg {
            AppEvent::Swap(_swap_event) => {
                // ⚠️ Swap cache invalidation is now handled by swap_requests_worker
                // This ensures cache is cleared AFTER database update completes
            }
            AppEvent::UserMint(mint_event) => {
                // When a mint happens, invalidate that user's cache
                let user_address = mint_event.user.to_lowercase();
                let cache_key = format!("mint:{}", user_address);
                
                cache.invalidate(&cache_key).await;
                info!("🗑️  Invalidated mint cache for user: {} (UserMint event)", user_address);
            }
            _ => {
                // Other events don't affect mint eligibility
            }
        }
    }
}

/// Verify NFT mint eligibility
/// Returns true if:
/// 1. The NFT belongs to the user (user_address matches and received=true)
/// 2. All chips of this NFT belong to the user (user_address matches and received=true)
async fn verify_nft_mint_eligibility(
    pool: &PgPool,
    user_address: &str,
    nft_id: &str,
) -> Result<bool, sqlx::Error> {
    info!("Verifying mint eligibility: user={}, nft_id={}", user_address, nft_id);

    // Parse nft_id to i32
    let nft_id_num: i32 = nft_id.parse()
        .map_err(|e| sqlx::Error::Decode(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Failed to parse nft_id: {}", e)
        ))))?;

    // Step 1: Check if NFT belongs to the user and is_mint status
    let nft_record = sqlx::query!(
        r#"
        SELECT id, user_address, received, is_mint
        FROM nfts 
        WHERE id = $1
        "#,
        nft_id_num
    )
    .fetch_optional(pool)
    .await?;

    match nft_record {
        None => {
            warn!("NFT {} does not exist", nft_id);
            return Ok(false);
        }
        Some(nft) => {
            // Check if NFT belongs to user and is received
            if nft.user_address.is_none() {
                warn!("NFT {} has no owner", nft_id);
                return Ok(false);
            }
            
            let nft_owner = nft.user_address.unwrap().to_lowercase();
            if nft_owner != user_address {
                warn!("NFT {} belongs to {} not {}", nft_id, nft_owner, user_address);
                return Ok(false);
            }
            
            if !nft.received.unwrap_or(false) {
                warn!("NFT {} is not received yet", nft_id);
                return Ok(false);
            }
            
            // 🔒 Check is_mint status to prevent duplicate requests
            let is_mint_status = nft.is_mint;
            if is_mint_status == 1 {
                warn!("NFT {} is already being minted (is_mint=1)", nft_id);
                return Ok(false);
            }
            if is_mint_status == 2 {
                warn!("NFT {} has already been minted (is_mint=2)", nft_id);
                return Ok(false);
            }
            
            info!("✅ NFT {} belongs to user {} and is ready to mint (is_mint={})", nft_id, user_address, is_mint_status);
        }
    }

    // Step 2: Check if all chips of this NFT belong to the user
    // Count total chips for this NFT
    let total_chips = sqlx::query!(
        "SELECT COUNT(*) as count FROM chips WHERE nft_id = $1",
        nft_id_num
    )
    .fetch_one(pool)
    .await?;
    
    let total_count = total_chips.count.unwrap_or(0);

    // Count chips owned by the user for this NFT
    let owned_chips = sqlx::query!(
        r#"
        SELECT COUNT(*) as count FROM chips 
        WHERE nft_id = $1 AND LOWER(user_address) = $2 AND received = true
        "#,
        nft_id_num,
        user_address
    )
    .fetch_one(pool)
    .await?;

    let owned_count = owned_chips.count.unwrap_or(0);

    info!(
        "NFT {} chips status: owned={}, total={}", 
        nft_id, owned_count, total_count
    );

    if total_count == 0 {
        warn!("NFT {} has no chips", nft_id);
        return Ok(false);
    }

    if owned_count != total_count {
        warn!(
            "NFT {} chips incomplete: user owns {}/{} chips", 
            nft_id, owned_count, total_count
        );
        return Ok(false);
    }

    info!("✅ All chips ({}) of NFT {} belong to user {}", total_count, nft_id, user_address);
    Ok(true)
}

/// Call safeMint function on NFT contract
async fn call_safe_mint_contract(
    contract_address: Address,
    to_address: Address,
    nft_id: String,
    uint256_param: u64,
    private_key: String,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    info!("Calling safeMint contract...");
    info!("  Contract: {:?}", contract_address);
    info!("  To: {:?}", to_address);
    info!("  NFT_id (tokenId string): {}", nft_id);
    info!("  Uint256 parameter: {}", uint256_param);

    // Parse private key
    let signer: PrivateKeySigner = private_key.parse()
        .map_err(|e: alloy::signers::local::LocalSignerError| format!("Failed to parse private key: {:?}", e))?;
    
    let wallet = EthereumWallet::from(signer);

    // Connect to RPC
    let rpc_url = "https://dream-rpc.somnia.network";
    let provider = ProviderBuilder::new()
        .wallet(wallet)
        .connect_http(rpc_url.parse()?);


    // Define contract ABI for safeMint function
    // Signature: safeMint(address,string,uint256)
    sol! {
        #[allow(missing_docs)]
        #[sol(rpc)]
        NFTContract,
        r#"[
            {
                "inputs": [
                    {"internalType": "address", "name": "to", "type": "address"},
                    {"internalType": "string", "name": "tokenId", "type": "string"},
                    {"internalType": "uint256", "name": "param", "type": "uint256"}
                ],
                "name": "safeMint",
                "outputs": [],
                "stateMutability": "nonpayable",
                "type": "function"
            }
        ]"#
    }

    // Create contract instance
    let contract = NFTContract::new(contract_address, provider);

    // Call safeMint with uint256 parameter
    info!("Sending safeMint transaction with parameters:");
    info!("  - to: {:?}", to_address);
    info!("  - tokenId (string): {}", nft_id);
    info!("  - uint256 param: {}", uint256_param);
    
    use alloy::primitives::U256;
    let uint256_value = U256::from(uint256_param);
    
    let tx_builder = contract.safeMint(to_address, nft_id.clone(), uint256_value);
    
    let pending_tx = tx_builder.send().await
        .map_err(|e| {
            error!("❌ Transaction failed with error: {:?}", e);
            error!("   tokenId: {}", nft_id);
            error!("   uint256 param: {}", uint256_param);
            format!("Failed to send transaction: {:?}", e)
        })?;
    
    let tx_hash = *pending_tx.tx_hash();
    info!("Transaction hash: {:?}", tx_hash);

    // Wait for confirmation (optional, can be commented out for faster response)
    info!("Waiting for transaction confirmation...");
    let receipt = pending_tx.get_receipt().await
        .map_err(|e| format!("Failed to get receipt: {:?}", e))?;
    
    info!("Transaction confirmed in block: {:?}", receipt.block_number);
    
    Ok(format!("{:?}", tx_hash))
}

/// Update NFT mint status in database
async fn update_nft_mint_status(
    pool: &PgPool,
    user_address: &str,
    nft_id: &str,
    is_mint: i32,
) -> Result<(), sqlx::Error> {
    info!("Updating NFT mint status: user={}, nft_id={}, is_mint={}", user_address, nft_id, is_mint);

    // Parse nft_id to i32 for database query
    let nft_id_num: i32 = nft_id.parse()
        .map_err(|e| sqlx::Error::Decode(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Failed to parse nft_id: {}", e)
        ))))?;

    // Update the nfts table
    let result = sqlx::query!(
        r#"
        UPDATE nfts 
        SET is_mint = $1
        WHERE LOWER(user_address) = $2 AND id = $3
        "#,
        is_mint,
        user_address.to_lowercase(),
        nft_id_num
    )
    .execute(pool)
    .await?;

    if result.rows_affected() > 0 {
        info!("✅ Updated {} NFT record(s)", result.rows_affected());
    } else {
        warn!("⚠️  No NFT records were updated for user: {} nft_id: {}", user_address, nft_id);
    }

    Ok(())
}

// ========================================
// 图片代理服务
// ========================================

/// Serve large image: GET /api/images/{file_name}
async fn serve_image(Path(file_name): Path<String>) -> Response {
    // Load base path from env
    dotenv::dotenv().ok();
    let base_path = std::env::var("IMAGES_BASE_PATH")
        .unwrap_or_else(|_| "/Users/martin/Downloads/workspace/rust/picture-cut/images".to_string());
    
    let file_path = std::path::Path::new(&base_path).join(&file_name);
    
    info!("Serving image: {:?}", file_path);
    
    // Check if file exists
    if !file_path.exists() {
        error!("Image not found: {:?}", file_path);
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "Image not found",
                "path": file_path.to_string_lossy()
            }))
        ).into_response();
    }
    
    // Open file
    let file = match File::open(&file_path).await {
        Ok(file) => file,
        Err(e) => {
            error!("Failed to open image file: {:?}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "Failed to open file",
                    "details": e.to_string()
                }))
            ).into_response();
        }
    };
    
    // Determine content type from file extension
    let ext = file_path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    
    let content_type = match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "image/jpeg",
    };
    
    // Create response with proper headers
    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);
    
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*"),
            (header::CACHE_CONTROL, "public, max-age=3600"),
        ],
        body
    ).into_response()
}

/// Serve tile image: GET /api/tiles/{file_name}/{tile_name}
/// Example: /api/tiles/1/1_0.png -> output/1/1_0.png
async fn serve_tile(Path((file_name, tile_name)): Path<(String, String)>) -> Response {
    // Load base path from env
    dotenv::dotenv().ok();
    let base_path = std::env::var("TILES_BASE_PATH")
        .unwrap_or_else(|_| "/Users/martin/Downloads/workspace/rust/picture-cut/output".to_string());
    
    // Build path: output/{file_name}/{tile_name} (no "tiles" subdirectory)
    let file_path = std::path::Path::new(&base_path)
        .join(&file_name)
        .join(&tile_name);
    
    info!("Serving tile: {:?}", file_path);
    
    // Check if file exists
    if !file_path.exists() {
        error!("Tile not found: {:?}", file_path);
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "Tile not found",
                "path": file_path.to_string_lossy(),
                "file_name": file_name,
                "tile_name": tile_name
            }))
        ).into_response();
    }
    
    // Open file
    let file = match File::open(&file_path).await {
        Ok(file) => file,
        Err(e) => {
            error!("Failed to open tile file: {:?}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "Failed to open file",
                    "details": e.to_string()
                }))
            ).into_response();
        }
    };
    
    // Create response with proper headers (tiles are always PNG)
    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);
    
    (
        [
            (header::CONTENT_TYPE, "image/png"),
            (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*"),
            (header::CACHE_CONTROL, "public, max-age=3600"),
        ],
        body
    ).into_response()
}

/// Get NFT user chips info: GET /api/nft-user-chips?nft_id={id}&user_address={address}
async fn get_nft_user_chips(
    Query(params): Query<NftUserChipsQuery>,
    State(state): State<Arc<AppStatus>>,
) -> Json<NftUserChipsResponse> {
    let nft_id = params.nft_id;
    let user_address = params.user_address.to_lowercase();
    info!("Querying chips for NFT ID: {}, user: {}", nft_id, user_address);
    
    // Query NFT info
    let nft_info = sqlx::query!(
        r#"
        SELECT file_name
        FROM nfts
        WHERE id = $1
        "#,
        nft_id
    )
    .fetch_optional(&state.db_pool)
    .await;
    
    let file_name = match nft_info {
        Ok(Some(record)) => record.file_name,
        Ok(None) => {
            warn!("NFT {} not found", nft_id);
            None
        }
        Err(e) => {
            error!("Failed to fetch NFT info: {:?}", e);
            None
        }
    };
    
    // Query chips info for this NFT that belong to the user (received = true)
    let chips = sqlx::query_as!(
        ChipInfo,
        r#"
        SELECT 
            id,
            x,
            y,
            w,
            h,
            file_name
        FROM chips
        WHERE nft_id = $1 
          AND LOWER(user_address) = $2
          AND received = true
        ORDER BY id
        "#,
        nft_id,
        user_address
    )
    .fetch_all(&state.db_pool)
    .await
    .unwrap_or_else(|e| {
        error!("Failed to fetch chips for NFT {} and user {}: {:?}", nft_id, user_address, e);
        vec![]
    });
    
    info!("Found {} received chips for NFT {} owned by user {}", chips.len(), nft_id, user_address);
    
    Json(NftUserChipsResponse {
        nft_id,
        user_address,
        file_name,
        chips,
    })
}

/// Get NFT user chips with base64 images: POST /api/nft-user-chips-batch
async fn get_nft_user_chips_batch(
    State(state): State<Arc<AppStatus>>,
    axum::extract::Json(request): axum::extract::Json<NftUserChipsBatchRequest>,
) -> Json<NftUserChipsBatchResponse> {
    let nft_id = request.nft_id;
    let user_address = request.user_address.to_lowercase();
    info!("Batch querying chips for NFT ID: {}, user: {}", nft_id, user_address);
    
    // Query NFT info
    let nft_info = sqlx::query!(
        r#"
        SELECT file_name
        FROM nfts
        WHERE id = $1
        "#,
        nft_id
    )
    .fetch_optional(&state.db_pool)
    .await;
    
    let file_name = match nft_info {
        Ok(Some(record)) => record.file_name,
        Ok(None) => {
            warn!("NFT {} not found", nft_id);
            None
        }
        Err(e) => {
            error!("Failed to fetch NFT info: {:?}", e);
            None
        }
    };
    
    // Query chips info for this NFT that belong to the user (received = true)
    let chips = sqlx::query_as!(
        ChipInfo,
        r#"
        SELECT 
            id,
            x,
            y,
            w,
            h,
            file_name
        FROM chips
        WHERE nft_id = $1 
          AND LOWER(user_address) = $2
          AND received = true
        ORDER BY id
        "#,
        nft_id,
        user_address
    )
    .fetch_all(&state.db_pool)
    .await
    .unwrap_or_else(|e| {
        error!("Failed to fetch chips for NFT {} and user {}: {:?}", nft_id, user_address, e);
        vec![]
    });
    
    info!("Found {} received chips for NFT {} owned by user {}", chips.len(), nft_id, user_address);
    
    // IPFS configuration
    dotenv::dotenv().ok();
    let ipfs_gateway = std::env::var("IPFS_GATEWAY")
        .unwrap_or_else(|_| "https://nftstorage.link/ipfs".to_string());
    let ipfs_cid = std::env::var("IPFS_IMAGE_CID")
        .unwrap_or_else(|_| "QmeepvJ75VyRyT2ewLeuYdGvPezSX9mru75LWpNFLPRvmE".to_string());
    
    // Convert chips to include base64 images
    let mut chips_with_base64 = Vec::new();
    
    for chip in chips {
        let base64_data = if let Some(ref chip_file_name) = chip.file_name {
            // Build IPFS URL: https://nftstorage.link/ipfs/CID/file_name
            let ipfs_url = format!("{}/{}/{}", ipfs_gateway, ipfs_cid, chip_file_name);
            
            info!("Fetching chip image from IPFS: {}", ipfs_url);
            
            // Fetch image from IPFS gateway
            match reqwest::get(&ipfs_url).await {
                Ok(response) => {
                    match response.bytes().await {
                        Ok(file_data) => {
                            use base64::{Engine as _, engine::general_purpose};
                            let base64_string = general_purpose::STANDARD.encode(&file_data);
                            Some(format!("data:image/png;base64,{}", base64_string))
                        }
                        Err(e) => {
                            warn!("Failed to read chip image data from IPFS {}: {:?}", ipfs_url, e);
                            None
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to fetch chip image from IPFS {}: {:?}", ipfs_url, e);
                    None
                }
            }
        } else {
            None
        };
        
        chips_with_base64.push(ChipInfoWithBase64 {
            id: chip.id,
            x: chip.x,
            y: chip.y,
            w: chip.w,
            h: chip.h,
            file_name: chip.file_name,
            base64: base64_data,
        });
    }
    
    info!("Successfully loaded {} chips with base64 images", chips_with_base64.len());
    
    Json(NftUserChipsBatchResponse {
        nft_id,
        user_address,
        file_name,
        chips: chips_with_base64,
    })
}
