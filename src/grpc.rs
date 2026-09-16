use futures::stream::StreamExt;
use futures::SinkExt;
use yellowstone_grpc_client::{Backoff, GeyserGrpcClient, ReconnectConfig, ReconnectionPolicy};
use yellowstone_grpc_proto::geyser::{
    CommitmentLevel, SubscribeRequest, SubscribeRequestFilterTransactions,
};
use std::{collections::HashMap, time::Duration};
use crate::decode::decode_transfer;
use shipstern_core::instruction::InstructionUpdate;
use yellowstone_grpc_proto::geyser::subscribe_update::UpdateOneof;
use crate::db::write_transfer;

const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";

pub async fn connect(endpoint: &str) -> Result<GeyserGrpcClient, Box<dyn std::error::Error>> {
    let reconnect_config = ReconnectConfig {
        backoff: Backoff::new(Duration::from_millis(200), 2.0, 6),
        policy: ReconnectionPolicy::RecoverMissedData {
            slot_retention: yellowstone_grpc_client::DEFAULT_SLOT_RETENTION, 
        },
    };

    let client = GeyserGrpcClient::build_from_shared(endpoint.to_string())?
        .x_token(None::<String>)?
        .set_reconnect_config(reconnect_config)
        .connect()
        .await?;

    Ok(client)
}

pub fn build_subscribe_request(from_slot: Option<u64>) -> SubscribeRequest {
    let mut transactions_filter = HashMap::new();
    transactions_filter.insert(
        "token_transfers".to_string(),
        SubscribeRequestFilterTransactions {
            account_include: vec![TOKEN_PROGRAM_ID.to_string()],
            vote: Some(false),
            failed: Some(false),
            ..Default::default()
        },
    );

    SubscribeRequest {
        transactions: transactions_filter,
        commitment: Some(CommitmentLevel::Confirmed as i32),
        from_slot,
        ..Default::default()
    }
}

pub async fn subscribe_and_decode(
    mut client: GeyserGrpcClient,
    pool: &sqlx::PgPool,
    from_slot: Option<u64>,
) -> Result<(), Box<dyn std::error::Error>> {
    let (mut subscribe_tx, mut stream) = client.subscribe().await?;

    subscribe_tx.send(build_subscribe_request(from_slot)).await?;

    println!("Subscribed. Waiting for messages...");

    while let Some(message) = stream.next().await {
        let update = match message {
            Ok(update) => update,
            Err(e) => {
                eprintln!("Stream error: {:?}", e);
                break;
            }
        };

        let Some(UpdateOneof::Transaction(tx_update)) = update.update_oneof else {
            continue;
        };

        let slot = tx_update.slot;

        let instructions = match InstructionUpdate::build_from_txn(&tx_update) {
            Ok(ixs) => ixs,
            Err(e) => {
                eprintln!("build_from_txn error: {:?}", e);
                continue;
            }
        };

        for (index, ix) in instructions.iter().enumerate() {
            if let Some(transfer) = decode_transfer(ix).await {
                let signature = bs58::encode(&ix.shared.signature).into_string();

                if let Err(e) =
                    write_transfer(pool, slot as i64, &signature, index as i16, &transfer).await
                {
                    eprintln!("write_transfer error: {:?}", e);
                }
            }
        }
    }
    Ok(())
}
