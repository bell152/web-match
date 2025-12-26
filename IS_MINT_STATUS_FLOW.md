# is_mint 状态设置流程分析

## 📋 状态定义

```rust
// is_mint 字段含义
// 0: 未申请 (not applied)
// 1: 申请中 (applying) 
// 2: 已mint (minted)
```

---

## 🔍 is_mint = 1 的设置时机

### **位置 1: `/api/user-safe-mint` 接口** (`router.rs:959-961`)

**函数**: `user_safe_mint`  
**模式**: 后端代付模式（Backend Pays）  
**路由**: `POST /api/user-safe-mint`

```rust
async fn user_safe_mint(
    State(state): State<Arc<AppStatus>>,
    axum::extract::Json(request): axum::extract::Json<UserSafeMintRequest>,
) -> Json<UserSafeMintResponse> {
    // ... 验证逻辑 ...
    
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
                // ...
            });
        }
    }
    
    // 🔄 Step 3: Call contract safeMint function
    info!("Step 3: Calling smart contract safeMint");
    match call_safe_mint_contract(...).await {
        Ok(tx_hash) => {
            // ✅ 成功：等待链上事件更新 is_mint = 2
        }
        Err(e) => {
            // ❌ 失败：回滚 is_mint = 0
            update_nft_mint_status(&state.db_pool, &user_address, &nft_id, 0).await;
        }
    }
}
```

**执行流程**：
1. ✅ 验证用户资格
2. ✅ **设置 `is_mint = 1`**（申请中）
3. ✅ 调用合约 `safeMint`
4. ✅ 成功 → 等待链上事件 → `is_mint = 2`
5. ❌ 失败 → 回滚 → `is_mint = 0`

---

### **位置 2: `/api/verify-mint-eligibility` 接口** (`router.rs:1117-1119`)

**函数**: `verify_mint_eligibility_api`  
**模式**: 用户自付模式（User Pays）  
**路由**: `POST /api/verify-mint-eligibility`

```rust
async fn verify_mint_eligibility_api(
    State(state): State<Arc<AppStatus>>,
    axum::extract::Json(request): axum::extract::Json<UserSafeMintRequest>,
) -> Json<MintEligibilityResponse> {
    // ... 验证逻辑 ...
    
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
                // ...
            });
        }
    }
    
    // Step 4: Return contract parameters for frontend to execute
    Json(MintEligibilityResponse {
        eligible: true,
        message: "You can proceed with minting. Use your wallet to call the contract.".to_string(),
        contract_address: Some("0x8557aFC94164F53a0828EB4ca16afE7dE280BE34".to_string()),
        token_id: Some(nft_id),
        uint256_param: Some(uint256_param),
    })
}
```

**执行流程**：
1. ✅ 验证用户资格
2. ✅ **设置 `is_mint = 1`**（申请中）
3. ✅ 返回合约参数给前端
4. ✅ 前端调用合约（用户钱包）
5. ✅ 成功 → 链上事件 → `is_mint = 2`
6. ❌ 失败 → `/api/mint-failed` → `is_mint = 0`

---

## 🔄 完整状态流转图

```
初始状态: is_mint = 0 (未申请)
        ↓
用户调用 /api/verify-mint-eligibility 或 /api/user-safe-mint
        ↓
设置 is_mint = 1 (申请中) ✅
        ↓
    ┌────┴────┐
    ↓         ↓
调用合约    用户取消/失败
    ↓         ↓
成功 ✅    失败 ❌
    ↓         ↓
链上事件    回滚 is_mint = 0
    ↓
UserMint 事件监听
    ↓
设置 is_mint = 2 (已mint) ✅
```

---

## 📊 状态设置位置汇总

| 状态值 | 设置位置 | 触发条件 |
|--------|---------|---------|
| **is_mint = 0** | `update_nft_mint_status(..., 0)` | 1. 初始状态<br>2. 回滚（合约调用失败）<br>3. `/api/mint-failed` |
| **is_mint = 1** | `update_nft_mint_status(..., 1)` | 1. `/api/user-safe-mint` (Step 2)<br>2. `/api/verify-mint-eligibility` (Step 3) |
| **is_mint = 2** | `process_user_mint_event` | 链上 `UserMint` 事件触发 |

---

## 🔍 关键函数

### **update_nft_mint_status** (`router.rs:1741-1765`)

```rust
async fn update_nft_mint_status(
    pool: &PgPool,
    user_address: &str,
    nft_id: &str,
    is_mint: i32,  // 0, 1, 或 2
) -> Result<(), sqlx::Error> {
    info!("Updating NFT mint status: user={}, nft_id={}, is_mint={}", 
        user_address, nft_id, is_mint);

    let nft_id_num: i32 = nft_id.parse()?;

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

    if result.rows_affected() == 0 {
        warn!("No NFT found to update: user={}, nft_id={}", user_address, nft_id);
    }

    Ok(())
}
```

---

## 🛡️ 状态保护机制

### **防止重复申请** (`router.rs:1595-1600`)

```rust
// 🔒 Check is_mint status to prevent duplicate requests
let is_mint_status = nft.is_mint;
if is_mint_status == 1 {
    warn!("NFT {} is already being minted (is_mint=1)", nft_id);
    return Ok(false);  // 拒绝重复申请
}
if is_mint_status == 2 {
    warn!("NFT {} has already been minted (is_mint=2)", nft_id);
    return Ok(false);  // 拒绝重复 mint
}
```

**作用**：
- ✅ 防止同一 NFT 被多次申请 mint
- ✅ 防止已 mint 的 NFT 再次 mint

---

## 📝 日志输出示例

### **设置 is_mint = 1**

```
INFO  Step 2: Updating database status to 'applying' (is_mint=1)
INFO  Updating NFT mint status: user=0xd693...4510, nft_id=12, is_mint=1
INFO  ✅ Updated NFT is_mint status to 1 for nft_id: 12
```

### **回滚 is_mint = 0**

```
WARN  Attempting to rollback database status due to contract call failure
INFO  Updating NFT mint status: user=0xd693...4510, nft_id=12, is_mint=0
INFO  ✅ Rolled back NFT is_mint status to 0
```

### **最终设置 is_mint = 2**

```
INFO  💎 Received UserMint event: ...
INFO  Processing UserMint event for user: 0xd693...4510
INFO  ✅ Updated NFT is_mint status to 2 for nft_id: 12
```

---

## ✅ 总结

| 问题 | 答案 |
|------|------|
| **is_mint = 1 什么时候设置？** | 在两个 API 接口中：<br>1. `/api/user-safe-mint` (后端代付)<br>2. `/api/verify-mint-eligibility` (用户自付) |
| **设置时机** | 在调用合约之前，先更新数据库状态 |
| **目的** | 防止重复申请，标记 NFT 正在 mint 中 |
| **回滚机制** | 如果合约调用失败，回滚到 `is_mint = 0` |
| **最终状态** | 链上 `UserMint` 事件触发后，设置为 `is_mint = 2` |

---

**实现位置**: `src/routers/router.rs`  
- `user_safe_mint`: 第 959-961 行  
- `verify_mint_eligibility_api`: 第 1117-1119 行  
- `update_nft_mint_status`: 第 1741-1765 行

