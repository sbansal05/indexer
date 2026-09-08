// indexer/src/main.rs

mod grpc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pool = db::connect_db("postgres://indexer:devpassword@127.0.0.1.5432/indexer_db").await?;
    let watermark = db::read_watermark(&pool).await?;
    print!("watermark on startup: {:?}", watermark);
    
    let client = grpc::connect("http://127.0.0.1:10000").await?;
    grpc::subscribe_and_print(client).await?;
    Ok(())
}