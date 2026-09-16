use shipstern_core::instruction::InstructionUpdate;
use shipstern_core::Parser;
use shipstern_spl_token_parser::instruction::Instruction;
use shipstern_spl_token_parser::InstructionParser;

#[derive(Debug)]
pub struct DecodedTransfer {
    pub source: String,
    pub destination: String,
    pub authority: String,
    pub mint: Option<String>,
    pub amount: u64,
    pub decimals: Option<i16>,
}

pub async fn decode_transfer(ix_update: &InstructionUpdate) -> Option<DecodedTransfer> {
    let parsed = match InstructionParser.parse(ix_update).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("decode error: {:?}", e);
            return None;
        }
    };

    match parsed.instruction? {
        Instruction::Transfer(t) => Some(DecodedTransfer {
            source: t.accounts.source.to_string(),
            destination: t.accounts.destination.to_string(),
            authority: t.accounts.owner.to_string(),
            mint: None,
            amount: t.args.amount,
            decimals: None,
        }),
        Instruction::TransferChecked(t) => Some(DecodedTransfer {
            source: t.accounts.source.to_string(),
            destination: t.accounts.destination.to_string(),
            authority: t.accounts.owner.to_string(),
            mint: Some(t.accounts.mint.to_string()),
            amount: t.args.amount,
            decimals: Some(t.args.decimals as i16),
        }),
        _=> None, 
    }

}