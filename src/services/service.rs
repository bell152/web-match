use tracing::{info, warn, error};
use bigdecimal::BigDecimal;
use chrono::Utc;
use sqlx::PgPool;
use std::str::FromStr;
use crate::entitys::entity::KlineUpdateEvent;
use alloy::providers::ProviderBuilder;
use alloy::primitives::Address;
use alloy::sol;
use std::collections::HashSet;

// ERC20 标准 balanceOf 函数
sol! {
    #[sol(rpc)]
    ERC20Token,
    r#"[
        {
            "inputs": [{"internalType": "address", "name": "account", "type": "address"}],
            "name": "balanceOf",
            "outputs": [{"internalType": "uint256", "name": "", "type": "uint256"}],
            "stateMutability": "view",
            "type": "function"
        }
    ]"#
}

/// Check if address is in blacklist (contract addresses that should not receive/revert chips)
fn is_blacklisted_address(address: &str) -> bool {
    dotenv::dotenv().ok();
    
    // 黑名单地址列表（合约地址）
    let blacklist_keys = vec![
        "NFT_CONTRACT",
        "QUOTER_ADDRESS", 
        "SWAP_EXECUTOR",
        "TOKEN_B",
        "TOKEN_A",
        "POOL_MANAGER",
        "HOOK_CONTRACT",
        "CURRENCY0_ADDRESS",
        "CURRENCY1_ADDRESS",
        "PUBLIC_KEY",
    ];
    
    // 收集所有黑名单地址（转为小写）
    let mut blacklist: HashSet<String> = HashSet::new();
    for key in blacklist_keys {
        if let Ok(addr) = std::env::var(key) {
            blacklist.insert(addr.to_lowercase());
        }
    }
    
    // 检查地址是否在黑名单中
    let addr_lower = address.to_lowercase();
    blacklist.contains(&addr_lower)
}

/// Query user's token balance from HakuToken contract
async fn query_token_balance(user_address: &str) -> Result<BigDecimal, Box<dyn std::error::Error>> {
    // Load config
    dotenv::dotenv().ok();
    
    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://dream-rpc.somnia.network".to_string());
    
    let token_address_str = std::env::var("TOKEN_B")
        .or_else(|_| std::env::var("CURRENCY1_ADDRESS"))
        .map_err(|_| "TOKEN_B or CURRENCY1_ADDRESS not set in .env")?;
    
    let token_address: Address = token_address_str.parse()?;
    let user_addr: Address = user_address.parse()?;
    
    info!("Querying balance for user: {} from token: {}", user_address, token_address);
    
    // Connect to RPC
    let provider = ProviderBuilder::new()
        .connect_http(rpc_url.parse()?);
    
    // Create contract instance
    let contract = ERC20Token::new(token_address, provider);
    
    // Call balanceOf
    let balance_uint = contract.balanceOf(user_addr).call().await?;
    
    let balance = BigDecimal::from_str(&balance_uint.to_string())?;
    
    info!("✅ Token balance query successful: {}", balance);
    
    Ok(balance)
}

pub async fn root() -> &'static str {
    info!( "method: {}", "root"  );
    "Hello, World!"
}


/// Insert swap request into database
pub async fn insert_swap_request(
    pool: &PgPool,
    data: (String, bool, String, String, i64, chrono::DateTime<Utc>),
) -> Result<i64, sqlx::Error> {
    dotenv::dotenv().ok();
    let token_decimals: i32 = std::env::var("TOKEN_DECIMALS").ok().and_then(|s| s.parse::<i32>().ok()).unwrap_or(18);
    let (user_address, zero_for_one, amount_in_raw, amount_out_raw, block_timestamp_raw, timestamp_utc) = data;

    // Convert String to BigDecimal
    let amount_in_bd = BigDecimal::from_str(&amount_in_raw)
        .map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
    let amount_out_bd = BigDecimal::from_str(&amount_out_raw)
        .map_err(|e| sqlx::Error::Decode(Box::new(e)))?;

    let rec = sqlx::query!(
        r#"
        INSERT INTO swap_requests (user_address, zero_for_one, amount_in_raw, amount_out_raw, token_decimals, block_timestamp_raw, timestamp_utc)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING id
        "#,
        user_address,
        zero_for_one,
        amount_in_bd,
        amount_out_bd,
        token_decimals,
        block_timestamp_raw,
        timestamp_utc
    )
    .fetch_one(pool)
    .await?;

    Ok(rec.id)
}

/// Receive chips logic (Transfer in)
/// Query user's token balance from HakuToken contract and receive new chips
pub async fn receive_chips(pool: &PgPool, user_address: &str, _value: &str) -> Result<(), sqlx::Error> {
    // 🚫 黑名单检查：合约地址不参与 chips 分配
    if is_blacklisted_address(user_address) {
        warn!("🚫 receive_chips: Skipping blacklisted address {}", user_address);
        return Ok(());
    }
    
    // Load env
    dotenv::dotenv().ok();
    // MAX_NFT_PER_USER is now the BATCH SIZE for acquiring new NFTs
    let batch_size_str = std::env::var("MAX_NFT_PER_USER").unwrap_or("3".to_string());
    let batch_size: i64 = batch_size_str.parse().unwrap_or(3);
    
    let token_decimals: u32 = std::env::var("TOKEN_DECIMALS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(18);

    info!("🟢 Receiving chips for user: {}", user_address);

    // ==================== Step 1: 查询链上 HakuToken 余额 ====================
    info!("Step 1: Querying HakuToken balance from blockchain...");
    
    let user_balance = match query_token_balance(user_address).await {
        Ok(balance) => balance,
        Err(e) => {
            error!("❌ Failed to query token balance for {}: {:?}", user_address, e);
            return Err(sqlx::Error::Decode(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to query token balance: {}", e)
            ))));
        }
    };
    
    info!("User {} token balance (raw): {}", user_address, user_balance);
    
    // 转换为可读格式并向下取整，得到应该拥有的 chips 数量
    let divisor = BigDecimal::from(10u64.pow(token_decimals));
    let balance_divided = &user_balance / &divisor;
    
    // 向下取整 (floor)
    let total_wallet_count = balance_divided.to_string()
        .split('.')
        .next()
        .unwrap_or("0")
        .parse::<i64>()
        .unwrap_or(0);
    
    info!("✅ User should have {} chips based on token balance (floor)", total_wallet_count);

    // ==================== Step 2: 查询数据库中已领取的 chips ====================
    info!("Step 2: Querying received chips from database...");
    
    let received_chips = sqlx::query!(
        r#"
        SELECT COUNT(*) as count
        FROM chips
        WHERE LOWER(user_address) = $1 AND received = true
        "#,
        user_address.to_lowercase()
    )
    .fetch_one(pool)
    .await?;

    let n_received = received_chips.count.unwrap_or(0);
    info!("✅ User {} has already received {} chips (N)", user_address, n_received);

    // ==================== Step 3: 计算需要新增的 chips ====================
    info!("Step 3: Calculating chips to receive...");
    
    let total_needed = total_wallet_count - n_received;

    // 🔑 添加单次领取上限，防止死循环
    const MAX_CHIPS_PER_RECEIVE: i64 = 1000;
    const LARGE_SYNC_THRESHOLD: i64 = 10000; // 超过此值认为是异常数据
    
    if total_needed > LARGE_SYNC_THRESHOLD {
        error!("🚨 Abnormal chip sync detected! User needs {} chips. This suggests data inconsistency.", total_needed);
        error!("   Possible causes:");
        error!("   1. Database was reset but chain state remains");
        error!("   2. Test data with large token balance");
        error!("   3. Historical accumulated imbalance");
        error!("   Recommendation: Manually fix database or reset test data");
        warn!("⚠️ Limiting to {} chips to prevent system overload", MAX_CHIPS_PER_RECEIVE);
    }
    
    let mut n_needed_receive = if total_needed > MAX_CHIPS_PER_RECEIVE {
        warn!("⚠️ User needs {} chips total, but limiting to {} per transaction", 
            total_needed, MAX_CHIPS_PER_RECEIVE);
        MAX_CHIPS_PER_RECEIVE
    } else {
        total_needed
    };

    info!("📊 Calculation:");
    info!("  Token balance chips (floor): {}", total_wallet_count);
    info!("  Currently received chips: {}", n_received);
    info!("  Total chips needed: {}", total_needed);
    info!("  Chips to receive (limited): {}", n_needed_receive);

    if n_needed_receive <= 0 {
        info!("No new chips to receive for user {}", user_address);
        return Ok(());
    }

    info!("User {} will receive {} chips this time (Batch Size: {})", user_address, n_needed_receive, batch_size);

    let mut tx = pool.begin().await?;

    // Strategy: Loop until satisfied
    // 1. Try to fulfill N chips from ALL currently owned NFTs (randomly distributed).
    // 2. If N > 0, acquire `batch_size` NEW NFTs.
    // 3. Loop back to 1. 
    
    // 🔑 添加最大循环次数限制，防止死循环
    const MAX_LOOP_ITERATIONS: i32 = 100; // 最多循环 100 次
    let mut loop_count = 0;

    loop {
        if n_needed_receive <= 0 {
            break;
        }
        
        // 检查循环次数
        loop_count += 1;
        if loop_count > MAX_LOOP_ITERATIONS {
            error!("🚨 Reached maximum loop iterations ({}). Still need {} chips.", MAX_LOOP_ITERATIONS, n_needed_receive);
            error!("   This suggests data inconsistency. Stopping to prevent infinite loop.");
            break;
        }

        // --- Step 1: Try to grab chips from owned NFTs ---
        // Find chips belonging to user's NFTs that are not yet received
        let available_chips = sqlx::query!(
            r#"
            SELECT id FROM chips 
            WHERE nft_id IN (SELECT id FROM nfts WHERE user_address = $1 AND received = true) 
            AND received = false 
            ORDER BY RANDOM() 
            LIMIT $2
            FOR UPDATE SKIP LOCKED
            "#,
            user_address,
            n_needed_receive
        )
        .fetch_all(&mut *tx)
        .await?;

        let chips_found = available_chips.len() as i64;

        if chips_found > 0 {
            // 批量更新优化
            let chip_ids: Vec<i32> = available_chips.iter().map(|c| c.id).collect();
            
            if !chip_ids.is_empty() {
                sqlx::query!(
                    "UPDATE chips SET user_address = $1, received = true WHERE id = ANY($2)",
                    user_address,
                    &chip_ids
                )
                .execute(&mut *tx)
                .await?;
                
                info!("🚀 Batch updated {} chips from owned NFTs", chip_ids.len());
            }
            
            n_needed_receive -= chips_found;
            info!("User {} filled {} chips from owned NFTs. Remaining needed: {}", user_address, chips_found, n_needed_receive);
        }

        if n_needed_receive <= 0 {
            break;
        }

        // --- Step 2: Acquire new batch of NFTs ---
        info!("Current NFTs exhausted. Attempting to acquire a new batch of {} NFTs...", batch_size);
        
        let new_nfts = sqlx::query!(
            r#"
            SELECT id FROM nfts 
            WHERE received = false 
            ORDER BY RANDOM() 
            LIMIT $1 
            FOR UPDATE SKIP LOCKED
            "#,
            batch_size
        )
        .fetch_all(&mut *tx)
        .await?;

        let nfts_acquired = new_nfts.len() as i64;

        if nfts_acquired == 0 {
            warn!("System ran out of available NFTs! User {} still needs {} chips.", user_address, n_needed_receive);
            break;
        }

        // 批量更新 NFTs 优化
        let nft_ids: Vec<i32> = new_nfts.iter().map(|n| n.id).collect();
        
        if !nft_ids.is_empty() {
            sqlx::query!(
                "UPDATE nfts SET user_address = $1, received = true WHERE id = ANY($2)",
                user_address,
                &nft_ids
            )
            .execute(&mut *tx)
            .await?;
            
            info!("🚀 Batch acquired {} new NFTs for user {}", nft_ids.len(), user_address);
        }

        // Continue loop to fill chips from these newly acquired NFTs
    }

    if n_needed_receive > 0 {
        warn!("Transaction finished with partial fill. User {} missed {} chips.", user_address, n_needed_receive);
    } else {
        info!("User {} successfully received all chips.", user_address);
    }

    tx.commit().await?;
    Ok(())
}

/// Parse nft_id from mint_remark string
/// Supports multiple formats:
/// - Pure number: "12"
/// - Formatted: "MintNFT#12:tokenURL" or "12:tokenURL"
/// - With prefix: "MintNFT#12"
fn parse_nft_id_from_remark(remark: &str) -> Result<i32, Box<dyn std::error::Error>> {
    // Try direct parse first (pure number)
    if let Ok(nft_id) = remark.parse::<i32>() {
        return Ok(nft_id);
    }
    
    // Try format: "MintNFT#12:tokenURL" or "MintNFT#12"
    if let Some(hash_pos) = remark.find('#') {
        let after_hash = &remark[hash_pos + 1..];
        // Extract number before ':' if exists
        if let Some(colon_pos) = after_hash.find(':') {
            let nft_id_str = &after_hash[..colon_pos];
            return nft_id_str.parse::<i32>()
                .map_err(|e| format!("Failed to parse nft_id from '{}' (format: MintNFT#nft_id:tokenURL): {}", remark, e).into());
        } else {
            // Format: "MintNFT#12" (no colon)
            return after_hash.parse::<i32>()
                .map_err(|e| format!("Failed to parse nft_id from '{}' (format: MintNFT#nft_id): {}", remark, e).into());
        }
    }
    
    // Try format: "12:tokenURL" (no prefix)
    if let Some(colon_pos) = remark.find(':') {
        let nft_id_str = &remark[..colon_pos];
        return nft_id_str.parse::<i32>()
            .map_err(|e| format!("Failed to parse nft_id from '{}' (format: nft_id:tokenURL): {}", remark, e).into());
    }
    
    // If all parsing attempts fail, return error
    Err(format!("Unable to parse nft_id from remark: '{}'. Expected formats: '12', 'MintNFT#12:tokenURL', '12:tokenURL', or 'MintNFT#12'", remark).into())
}

/// Recycle chips for userMint transaction
/// When a user mints an NFT, recycle all chips associated with that NFT
/// Sets is_mint=2 and mint_user=user_address for all chips with matching nft_id
async fn recycle_chips_for_mint(
    pool: &PgPool,
    user_address: &str,
    nft_id_str: &str,  // mint_remark contains nft_id
) -> Result<(), sqlx::Error> {
    info!("🔄 Recycling chips for userMint: user={}, mint_remark={}", user_address, nft_id_str);
    
    // Parse nft_id from remark (supports multiple formats)
    let nft_id = match parse_nft_id_from_remark(nft_id_str) {
        Ok(id) => {
            info!("✅ Parsed nft_id: {} from remark: '{}'", id, nft_id_str);
            id
        }
        Err(e) => {
            error!("❌ Failed to parse nft_id from mint_remark '{}': {:?}", nft_id_str, e);
            return Err(sqlx::Error::Decode(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Invalid nft_id in mint_remark: {}", e)
            ))));
        }
    };
    
    let mut tx = pool.begin().await?;
    
    // ✅ 查找所有与该 nft_id 相关的 chips
    let chips_to_recycle = sqlx::query!(
        r#"
        SELECT id FROM chips
        WHERE nft_id = $1
        FOR UPDATE SKIP LOCKED
        "#,
        nft_id
    )
    .fetch_all(&mut *tx)
    .await?;
    
    let chip_count = chips_to_recycle.len();
    info!("Found {} chips to recycle for nft_id: {}", chip_count, nft_id);
    
    if chip_count == 0 {
        warn!("⚠️  No chips found for nft_id: {}", nft_id);
        tx.commit().await?;
        return Ok(());
    }
    
    // ✅ 批量更新：设置 is_mint=2, mint_user=user_address
    let chip_ids: Vec<i32> = chips_to_recycle.iter().map(|c| c.id).collect();
    
    sqlx::query!(
        r#"
        UPDATE chips 
        SET is_mint = 2, mint_user = $1
        WHERE id = ANY($2)
        "#,
        user_address.to_lowercase(),
        &chip_ids
    )
    .execute(&mut *tx)
    .await?;
    
    info!("✅ Recycled {} chips for userMint: user={}, nft_id={}", 
        chip_count, user_address, nft_id);
    
    tx.commit().await?;
    Ok(())
}

/// Revert chips logic (for Transfer out)
/// Query user's token balance from HakuToken contract and revert excess chips
/// If mint_remark is provided, recycle chips associated with that NFT
pub async fn revert_chips(
    pool: &PgPool, 
    user_address: &str, 
    _value: &str,
    mint_remark: Option<&str>,  // ✅ 新增：如果提供，说明是 userMint 交易
) -> Result<(), sqlx::Error> {
    // 🚫 黑名单检查：合约地址不参与 chips 分配
    if is_blacklisted_address(user_address) {
        warn!("🚫 revert_chips: Skipping blacklisted address {}", user_address);
        return Ok(());
    }
    
    // ==================== 互斥逻辑：根据 mint_remark 选择执行路径 ====================
    // ✅ 如果 mint_remark 有值，说明是 userMint 交易，执行 Mint revert logic
    // ✅ 如果 mint_remark 为空，说明是普通转账，执行 Transfer revert logic
    if let Some(remark) = mint_remark {
        // ========== Mint revert logic: 回收 userMint 相关的 chips ==========
        info!("🔄 Processing userMint transaction, recycling chips for nft_id: {}", remark);
        return recycle_chips_for_mint(pool, user_address, remark).await;
    } else {
        // ========== Transfer revert logic: 根据链上余额退回 chips ==========
        // Load env
        dotenv::dotenv().ok();
        let token_decimals: u32 = std::env::var("TOKEN_DECIMALS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(18);

        info!("🔴 Reverting chips for user: {}", user_address);

        // ==================== Step 1: 查询链上 HakuToken 余额 ====================
        info!("Step 1: Querying HakuToken balance from blockchain...");
        
        let user_balance = match query_token_balance(user_address).await {
            Ok(balance) => balance,
            Err(e) => {
                error!("❌ Failed to query token balance for {}: {:?}", user_address, e);
                return Err(sqlx::Error::Decode(Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to query token balance: {}", e)
                ))));
            }
        };
        
        info!("User {} token balance (raw): {}", user_address, user_balance);
        
        // 转换为可读格式并向下取整，得到应该拥有的 chips 数量
        let divisor = BigDecimal::from(10u64.pow(token_decimals));
        let balance_divided = &user_balance / &divisor;
        
        // 向下取整 (floor)
        let total_wallet_count = balance_divided.to_string()
            .split('.')
            .next()
            .unwrap_or("0")
            .parse::<i64>()
            .unwrap_or(0);
        
        info!("✅ User should have {} chips based on token balance (floor)", total_wallet_count);

        // ==================== Step 2: 查询数据库中已领取的 chips ====================
        info!("Step 2: Querying received chips from database...");
        
        let received_chips = sqlx::query!(
            r#"
            SELECT COUNT(*) as count
            FROM chips
            WHERE LOWER(user_address) = $1 AND received = true
            "#,
            user_address.to_lowercase()
        )
        .fetch_one(pool)
        .await?;

        let n_received = received_chips.count.unwrap_or(0);
        info!("✅ User {} has already received {} chips (N)", user_address, n_received);

        // ==================== Step 3: 计算需要退回的 chips ====================
        info!("Step 3: Calculating chips to revert...");
        
        let mut n_needed_revert = n_received - total_wallet_count;

        info!("📊 Calculation:");
        info!("  Token balance chips (floor): {}", total_wallet_count);
        info!("  Currently received chips: {}", n_received);
        info!("  Chips to revert: {}", n_needed_revert);

        if n_needed_revert <= 0 {
            info!("No new chips to revert for user {}", user_address);
            return Ok(());
        }
        
        info!("User {} needs to revert {} chips", user_address, n_needed_revert);
        let mut tx = pool.begin().await?;
        // Get all NFTs owned by user
        // Do not revert nfts whitch is minted by HakuNFTMint event
        let user_nfts = sqlx::query!(
            "SELECT id FROM nfts WHERE user_address = $1 AND received = true AND is_mint > 0 ORDER BY RANDOM()",
            user_address
        )
        .fetch_all(&mut *tx)
        .await?;
        for nft in user_nfts {
            if n_needed_revert <= 0 {
                break;
            }
            let nft_id = nft.id;
            // Count chips owned by user for this NFT (M)
            let chips_rec = sqlx::query!(
                "SELECT id FROM chips WHERE nft_id = $1 AND user_address = $2 AND received = true FOR UPDATE SKIP LOCKED",
                nft_id,
                user_address
            )
            .fetch_all(&mut *tx)
            .await?;
            let m_owned = chips_rec.len() as i64;
            if m_owned == 0 {
                continue;
            }
            if m_owned >= n_needed_revert {
                // Case 2: M >= N
                // Cancel N chips (批量更新优化)
                let chips_to_cancel = &chips_rec[0..n_needed_revert as usize];
                let chip_ids: Vec<i32> = chips_to_cancel.iter().map(|c| c.id).collect();
                
                if !chip_ids.is_empty() {
                    sqlx::query!(
                        "UPDATE chips SET user_address = NULL, received = false WHERE id = ANY($1)",
                        &chip_ids
                    )
                    .execute(&mut *tx)
                    .await?;
                    
                    info!("🚀 Batch updated {} chips for NFT {}", chip_ids.len(), nft_id);
                }
                // If M == N, cancel NFT
                if m_owned == n_needed_revert {
                    sqlx::query!(
                        "UPDATE nfts SET user_address = NULL, received = false WHERE id = $1",
                        nft_id
                    )
                    .execute(&mut *tx)
                    .await?;
                    info!("User {} reverted NFT {} (All chips reverted)", user_address, nft_id);
                }
                info!("User {} reverted {} chips from NFT {}", user_address, n_needed_revert, nft_id);
                n_needed_revert = 0;
            } else {
                // Case 3: M < N
                // Cancel all M chips (批量更新优化)
                let chip_ids: Vec<i32> = chips_rec.iter().map(|c| c.id).collect();
                
                if !chip_ids.is_empty() {
                    sqlx::query!(
                        "UPDATE chips SET user_address = NULL, received = false WHERE id = ANY($1)",
                        &chip_ids
                    )
                    .execute(&mut *tx)
                    .await?;
                    
                    info!("🚀 Batch updated {} chips for NFT {}", chip_ids.len(), nft_id);
                }
                // Cancel NFT (since all chips are gone)
                sqlx::query!(
                    "UPDATE nfts SET user_address = NULL, received = false WHERE id = $1",
                    nft_id
                )
                .execute(&mut *tx)
                .await?;

                info!("User {} reverted all {} chips from NFT {} (and the NFT itself)", user_address, m_owned, nft_id);
                n_needed_revert -= m_owned;
            }
        }
        if n_needed_revert > 0 {
            warn!("User {} did not have enough chips to revert. Remaining needed: {}", user_address, n_needed_revert);
        }
        tx.commit().await?;
        Ok(())
    }
}

/// Update K-line data
pub async fn update_kline(
    pool: &PgPool,
    data: (String, bool, String, String, i64, chrono::DateTime<Utc>),
) -> Result<Vec<KlineUpdateEvent>, sqlx::Error> {
    use crate::services::time_utils::get_kline_start_time; 
    
    let (_user_address, zero_for_one, amount_in_raw, amount_out_raw, _timestamp_raw, timestamp_utc) = data;
    
    // Load token decimals
    dotenv::dotenv().ok();
    let token_decimals: i32 = std::env::var("TOKEN_DECIMALS")
        .ok()
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(18);
    
    // Parse amounts
    let amount_in = BigDecimal::from_str(&amount_in_raw).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
    let amount_out = BigDecimal::from_str(&amount_out_raw).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
    
    // Convert to human-readable format (divide by 10^decimals)
    let divisor = BigDecimal::from(10u64.pow(token_decimals as u32));
    let amount_in_readable = &amount_in / &divisor;
    let amount_out_readable = &amount_out / &divisor;
    
    // Calculate price (TokenB / STT) using readable amounts
    // Assuming: 
    // zero_for_one = true  => STT -> TokenB (Input: STT, Output: TokenB) => Price = STT/TokenB = AmountIn / AmountOut
    // zero_for_one = false => TokenB -> STT (Input: TokenB, Output: STT) => Price = STT/TokenB = AmountOut / AmountIn
    
    let price = if zero_for_one {
        if amount_out_readable == BigDecimal::from(0) { 
            BigDecimal::from(0) 
        } else { 
            &amount_in_readable / &amount_out_readable 
        }
    } else {
        if amount_in_readable == BigDecimal::from(0) { 
            BigDecimal::from(0) 
        } else { 
            &amount_out_readable / &amount_in_readable 
        }
    };

    // Determine volume (use readable amounts)
    // volume_base (STT) and volume_quote (TokenB)
    // If zero_for_one (STT -> TokenB): base=AmountIn, quote=AmountOut
    // If !zero_for_one (TokenB -> STT): base=AmountOut, quote=AmountIn
    let (vol_base, vol_quote) = if zero_for_one {
        (amount_in_readable.clone(), amount_out_readable.clone())
    } else {
        (amount_out_readable.clone(), amount_in_readable.clone())
    };

    let intervals = vec!["1m", "5m", "15m", "1h", "4h", "1d"];
    let pair_id = 1; // Default pair ID for now

    let mut events = Vec::new();

    for interval in intervals {
        let start_time = get_kline_start_time(timestamp_utc, interval).naive_utc();
        
        // Upsert K-line
        let rec = sqlx::query!(
            r#"
            INSERT INTO kline (
                pair_id, interval, start_time, 
                open_price, high_price, low_price, close_price, 
                volume_base, volume_quote, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NOW())
            ON CONFLICT (pair_id, interval, start_time)
            DO UPDATE SET
                high_price = GREATEST(kline.high_price, EXCLUDED.high_price),
                low_price = LEAST(kline.low_price, EXCLUDED.low_price),
                close_price = EXCLUDED.close_price,
                volume_base = kline.volume_base + EXCLUDED.volume_base,
                volume_quote = kline.volume_quote + EXCLUDED.volume_quote,
                updated_at = NOW()
            RETURNING pair_id, interval, start_time, open_price, high_price, low_price, close_price, volume_base, volume_quote
            "#,
            pair_id,
            interval,
            start_time,
            price, // open
            price, // high
            price, // low
            price, // close
            vol_base,
            vol_quote
        )
        .fetch_one(pool)
        .await?;

        // Construct event
        events.push(KlineUpdateEvent {
            pair_id: rec.pair_id,
            interval: rec.interval,
            start_time: rec.start_time.and_utc().timestamp(),
            open: rec.open_price.to_string(),
            high: rec.high_price.to_string(),
            low: rec.low_price.to_string(),
            close: rec.close_price.to_string(),
            volume_base: rec.volume_base.to_string(),
            volume_quote: rec.volume_quote.to_string(),
        });
    }

    info!("Updated K-lines for timestamp {}", timestamp_utc);

    Ok(events)
}

/// Process UserMint event and update NFT status
/// Called when UserMint event is received from blockchain
pub async fn process_user_mint_event(
    pool: &PgPool,
    user_address: &str,
    token_id: &str,
    block_number: u64,
    remark: &str,
    token_url: &str,
) -> Result<(), sqlx::Error> {
    info!("Processing UserMint event: user={}, token_id={}, block_number={}, remark={}, token_url={}", 
        user_address, token_id, block_number, remark, token_url);

    // Parse remark as nft_id
    let nft_id: i32 = remark.parse()
        .map_err(|e| sqlx::Error::Decode(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Failed to parse remark as nft_id: {}", e)
        ))))?;

    // Parse token_id to i64 for database
    let token_id_num: i64 = token_id.parse()
        .map_err(|e| sqlx::Error::Decode(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Failed to parse token_id: {}", e)
        ))))?;

    // Parse block_number to i64
    let block_number_i64 = block_number as i64;

    // Update the NFT record (including token_url)
    let result = sqlx::query!(
        r#"
        UPDATE nfts 
        SET user_address = $1, 
            token_id = $2, 
            is_mint = 2,
            block_number = $3,
            token_url = $4
        WHERE id = $5
        "#,
        user_address.to_lowercase(),
        token_id_num,
        block_number_i64,
        token_url,
        nft_id
    )
    .execute(pool)
    .await?;

    if result.rows_affected() > 0 {
        info!("✅ Successfully updated NFT {} - is_mint=2 (mint successful), token_id={}, block_number={}, token_url={}", 
            nft_id, token_id, block_number, token_url);
    } else {
        warn!("⚠️  No NFT record found with id={} (remark={})", nft_id, remark);
    }

    Ok(())
}

/// User Transfer Worker - 处理 Token Transfer 事件
/// 
/// 关键参数需要确认：
/// - ❓ value 的单位是什么？raw value (带 18 位小数) 还是已转换的可读值？
/// - ❓ 是否需要检查转账金额的最小值？
/// - ❓ 是否需要记录转账历史到数据库？
/// - ❓ 转账是否会触发缓存失效？
/// - ❓ 其他业务逻辑？
/// Process Transfer event from blockchain
/// This function handles both sender (revert) and receiver (receive) logic
pub async fn process_transfer_event(
    pool: &PgPool,
    from_address: &str,
    to_address: &str,
    value: &str,
    mint_remark: Option<&str>,  // ✅ 新增：来自 HakuNFTMint 事件的 remark
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    
    info!("💸 Processing transfer event:");
    info!("  From: {}", from_address);
    info!("  To: {}", to_address);
    info!("  Value: {}", value);
    if let Some(remark) = mint_remark {
        info!("  Mint Remark: {} (userMint transaction)", remark);
    } else {
        info!("  Mint Remark: None (normal user transfer)");
    }
    
    // ==================== 处理 FROM 地址（转出方）====================
    // 转出意味着余额减少，执行 revert_chips
    info!("🔴 Start Processing sender (from): {}", from_address);
    
    if let Err(e) = revert_chips(pool, from_address, value, mint_remark).await {
        error!("❌ Failed to revert chips for sender {}: {:?}", from_address, e);
        return Err(Box::new(e));
    }
    
    info!("✅ Reverted completed! chips for sender: {}", from_address);
    
    // ==================== 处理 TO 地址（接收方）====================
    // 转入意味着余额增加，执行 receive_chips
    info!("🟢 Start Processing receiver (to): {}", to_address);
    
    // ❓ 问题 4: value 是否需要转换格式？
    // ❓ 问题 5: 接收是否有其他业务逻辑？
    
    if let Err(e) = receive_chips(pool, to_address, value).await {
        error!("❌ Failed to receive chips for receiver {}: {:?}", to_address, e);
        return Err(Box::new(e));
    }
    
    info!("✅ Received completed! chips for receiver: {}", to_address);
    
    // ❓ 问题 6: 是否需要记录这笔转账到数据库？
    // 例如：INSERT INTO transfers (from_address, to_address, value, ...) VALUES (...)
    
    // ❓ 问题 7: 是否需要触发缓存失效？
    // 例如：invalidate_cache_for_user(from_address)
    //       invalidate_cache_for_user(to_address)
    
    // ❓ 问题 8: 是否需要广播事件给前端？
    
    info!("✅ Transfer event processed successfully");
    Ok(())
}
