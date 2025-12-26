


use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;
use web_match::routers::router::app_map;





#[tokio::main]
async fn main()-> anyhow::Result<()>  {
    let subscriber = FmtSubscriber::builder().with_max_level(Level::INFO).finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");
    let port = 8686;
    let ip = "0.0.0.0";
    let address = format!("{}:{}",ip,port);
    let listener = tokio::net::TcpListener::bind(&address).await?;
    /*  dotenv().ok();
    let test_var = env::var("PAYPAL_CLIENT_ID").unwrap_or("".to_string());
    info!("test var: {}",test_var); */
    // sched.start().await;
    info!("listening on {}", &address);
    let app = app_map().await;
    axum::serve(listener, app).await?;
    
    Ok(())
}

