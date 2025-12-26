use alloy::primitives::{Address, Uint, Signed};
use tracing::info;

/// Pool 配置结构体
/// 对应 Solidity 的 PoolConfig 库
#[derive(Debug, Clone)]
pub struct PoolConfig {
    // ============ 合约地址 ============
    pub pool_manager: Address,
    pub token_a: Address,          // STT (原生币)
    pub token_b: Address,          // HakuToken
    pub currency0: Address,        // 同 token_a
    pub currency1: Address,        // 同 token_b
    pub swap_executor: Address,
    pub nft_contract: Address,     // NFT 合约地址
    
    // ============ RPC 配置 ============
    pub ws_url: String,            // WebSocket RPC URL
    
    // ============ Pool 参数 ============
    pub fee: u32,                  // 手续费率 (2999 = 0.2999%)
    pub tick_spacing: i32,         // tick 间距 (60)
    pub hooks: Address,            // hooks 合约地址
    
    // ============ 其他参数 ============
    pub sqrt_price_x96: String,    // 初始价格 (sqrtPriceX96 格式)
    pub pool_id: String,           // Pool ID
}

impl PoolConfig {
    /// 从环境变量加载 Pool 配置
    pub fn from_env() -> Result<Self, String> {
        dotenv::dotenv().ok();
        
        // 解析合约地址
        let pool_manager = std::env::var("POOL_MANAGER")
            .map_err(|_| "POOL_MANAGER not set")?
            .parse::<Address>()
            .map_err(|e| format!("Invalid POOL_MANAGER address: {}", e))?;
        
        let token_a = std::env::var("TOKEN_A")
            .map_err(|_| "TOKEN_A not set")?
            .parse::<Address>()
            .map_err(|e| format!("Invalid TOKEN_A address: {}", e))?;
        
        let token_b = std::env::var("TOKEN_B")
            .map_err(|_| "TOKEN_B not set")?
            .parse::<Address>()
            .map_err(|e| format!("Invalid TOKEN_B address: {}", e))?;
        
        // CURRENCY0 和 CURRENCY1 可以从 TOKEN_A/TOKEN_B 或独立配置读取
        let currency0 = std::env::var("CURRENCY0_ADDRESS")
            .unwrap_or_else(|_| std::env::var("TOKEN_A").unwrap())
            .parse::<Address>()
            .map_err(|e| format!("Invalid CURRENCY0_ADDRESS: {}", e))?;
        
        let currency1 = std::env::var("CURRENCY1_ADDRESS")
            .unwrap_or_else(|_| std::env::var("TOKEN_B").unwrap())
            .parse::<Address>()
            .map_err(|e| format!("Invalid CURRENCY1_ADDRESS: {}", e))?;
        
        let swap_executor = std::env::var("SWAP_EXECUTOR")
            .map_err(|_| "SWAP_EXECUTOR not set")?
            .parse::<Address>()
            .map_err(|e| format!("Invalid SWAP_EXECUTOR address: {}", e))?;
        
    
        
        let nft_contract = std::env::var("NFT_CONTRACT")
            .map_err(|_| "NFT_CONTRACT not set")?
            .parse::<Address>()
            .map_err(|e| format!("Invalid NFT_CONTRACT address: {}", e))?;
        
        // 解析 RPC 配置
        let ws_url = std::env::var("WS_URL")
            .map_err(|_| "WS_URL not set")?;
        
        // 解析 Pool 参数
        let fee = std::env::var("POOL_FEE")
            .map_err(|_| "POOL_FEE not set")?
            .parse::<u32>()
            .map_err(|e| format!("Invalid POOL_FEE: {}", e))?;
        
        let tick_spacing = std::env::var("POOL_TICK_SPACING")
            .map_err(|_| "POOL_TICK_SPACING not set")?
            .parse::<i32>()
            .map_err(|e| format!("Invalid POOL_TICK_SPACING: {}", e))?;
        
        let hooks = std::env::var("POOL_HOOKS")
            .map_err(|_| "POOL_HOOKS not set")?
            .parse::<Address>()
            .map_err(|e| format!("Invalid POOL_HOOKS address: {}", e))?;
        
        // 解析其他参数
        let sqrt_price_x96 = std::env::var("POOL_SQRT_PRICE_X96")
            .map_err(|_| "POOL_SQRT_PRICE_X96 not set")?;
        
        let pool_id = std::env::var("POOL_ID")
            .map_err(|_| "POOL_ID not set")?;
        
        let config = Self {
            pool_manager,
            token_a,
            token_b,
            currency0,
            currency1,
            swap_executor,
            nft_contract,
            ws_url,
            fee,
            tick_spacing,
            hooks,
            sqrt_price_x96,
            pool_id,
        };
        
        info!("✅ Pool 配置加载成功:");
        info!("  Pool Manager: {:?}", config.pool_manager);
        info!("  Token A (STT): {:?}", config.token_a);
        info!("  Token B (HakuToken): {:?}", config.token_b);
        info!("  Swap Executor: {:?}", config.swap_executor);
        info!("  NFT Contract: {:?}", config.nft_contract);
        info!("  WebSocket URL: {}", config.ws_url);
        info!("  Fee: {} ({}%)", config.fee, config.fee as f64 / 10000.0);
        info!("  Tick Spacing: {}", config.tick_spacing);
        info!("  Hooks: {:?}", config.hooks);
        info!("  Pool ID: {}", config.pool_id);
        
        Ok(config)
    }
    
    /// 获取 Alloy 格式的 fee (Uint<24, 1>)
    pub fn get_fee_uint24(&self) -> Uint<24, 1> {
        Uint::<24, 1>::from(self.fee)
    }
    
    /// 获取 Alloy 格式的 tick_spacing (Signed<24, 1>)
    pub fn get_tick_spacing_int24(&self) -> Result<Signed<24, 1>, String> {
        Signed::<24, 1>::try_from(self.tick_spacing)
            .map_err(|e| format!("Failed to convert tick_spacing: {}", e))
    }
    
    /// 验证配置的一致性
    pub fn validate(&self) -> Result<(), String> {
        // 验证 currency0 和 token_a 一致
        if self.currency0 != self.token_a {
            return Err(format!(
                "CURRENCY0 ({:?}) != TOKEN_A ({:?})", 
                self.currency0, self.token_a
            ));
        }
        
        // 验证 currency1 和 token_b 一致
        if self.currency1 != self.token_b {
            return Err(format!(
                "CURRENCY1 ({:?}) != TOKEN_B ({:?})", 
                self.currency1, self.token_b
            ));
        }
        
        // 验证 fee 范围 (0-100万，即 0%-100%)
        if self.fee > 1_000_000 {
            return Err(format!("Invalid fee: {} (must be <= 1000000)", self.fee));
        }
        
        // 验证 tick_spacing 范围
        if self.tick_spacing <= 0 || self.tick_spacing > 10000 {
            return Err(format!(
                "Invalid tick_spacing: {} (must be 1-10000)", 
                self.tick_spacing
            ));
        }
        
        Ok(())
    }
    
    /// 打印配置摘要
    pub fn print_summary(&self) {
        info!("==================== Pool 配置摘要 ====================");
        info!("合约地址:");
        info!("  Pool Manager   : {:?}", self.pool_manager);
        info!("  Token A (STT)  : {:?}", self.token_a);
        info!("  Token B (Haku) : {:?}", self.token_b);
        info!("  Swap Executor  : {:?}", self.swap_executor);
        info!("  NFT Contract   : {:?}", self.nft_contract);
        info!("");
        info!("RPC 配置:");
        info!("  WebSocket URL  : {}", self.ws_url);
        info!("");
        info!("Pool 参数:");
        info!("  Fee            : {} ({}%)", self.fee, self.fee as f64 / 10000.0);
        info!("  Tick Spacing   : {}", self.tick_spacing);
        info!("  Hooks          : {:?}", self.hooks);
        info!("");
        info!("其他:");
        info!("  Pool ID        : {}", self.pool_id);
        info!("======================================================");
    }
}

/// 获取全局 Pool 配置
/// 每次调用都从环境变量重新读取（简单但可靠）
pub fn get_pool_config() -> Result<PoolConfig, String> {
    let config = PoolConfig::from_env()?;
    config.validate()?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_pool_config_loading() {
        // 这个测试需要 .env 文件存在
        dotenv::dotenv().ok();
        
        match PoolConfig::from_env() {
            Ok(config) => {
                println!("配置加载成功:");
                config.print_summary();
                
                // 验证配置
                assert!(config.validate().is_ok());
                
                // 测试类型转换
                let fee_uint = config.get_fee_uint24();
                assert_eq!(fee_uint, Uint::<24, 1>::from(config.fee));
                
                let tick_spacing_int = config.get_tick_spacing_int24().unwrap();
                assert_eq!(tick_spacing_int, Signed::<24, 1>::try_from(config.tick_spacing).unwrap());
            }
            Err(e) => {
                println!("配置加载失败: {}", e);
                println!("请确保 .env 文件中包含所有必要的 Pool 配置参数");
            }
        }
    }
}

