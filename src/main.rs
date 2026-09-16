mod db;
mod grpc;
mod decode;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set — check your .env file");

    let pool = db::connect_db(&database_url).await?;
    let watermark = db::read_watermark(&pool).await?;
    println!("Watermark on startup: {:?}", watermark);

    let from_slot = watermark.map(|s| s as u64);

    let client = grpc::connect("http://127.0.0.1:10000").await?;
    grpc::subscribe_and_decode(client, &pool, from_slot).await?;
    Ok(())
}
