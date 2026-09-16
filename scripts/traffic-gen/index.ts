// scripts/traffic-gen/index.ts

import {
  Connection,
  Keypair,
  LAMPORTS_PER_SOL,
  sendAndConfirmTransaction,
  Transaction,
} from "@solana/web3.js";
import {
  createMint,
  getOrCreateAssociatedTokenAccount,
  mintTo,
  createTransferInstruction,
  createTransferCheckedInstruction,
} from "@solana/spl-token";

const RPC_URL = "http://127.0.0.1:8899";
const NUM_WALLETS = 4;
const TRANSFER_INTERVAL_MS = 2000;
const MINT_DECIMALS = 6;
const INITIAL_MINT_AMOUNT = 1_000_000 * 10 ** MINT_DECIMALS;

async function airdropAndConfirm(connection: Connection, pubkey: Keypair["publicKey"], sol: number) {
  const sig = await connection.requestAirdrop(pubkey, sol * LAMPORTS_PER_SOL);
  await connection.confirmTransaction(sig, "confirmed");
}

async function main() {
  const connection = new Connection(RPC_URL, "confirmed");

  console.log("Setting up payer...");
  const payer = Keypair.generate();
  await airdropAndConfirm(connection, payer.publicKey, 10);

  console.log("Creating test mint...");
  const mint = await createMint(connection, payer, payer.publicKey, null, MINT_DECIMALS);
  console.log(`Mint created: ${mint.toBase58()}`);

  console.log(`Creating ${NUM_WALLETS} test wallets...`);
  const wallets: Keypair[] = [];
  const tokenAccounts: Awaited<ReturnType<typeof getOrCreateAssociatedTokenAccount>>[] = [];
  const balances: number[] = new Array(NUM_WALLETS).fill(0);

  for (let i = 0; i < NUM_WALLETS; i++) {
    const wallet = Keypair.generate();
    await airdropAndConfirm(connection, wallet.publicKey, 1);
    const ata = await getOrCreateAssociatedTokenAccount(connection, payer, mint, wallet.publicKey);

    if (i === 0) {
      await mintTo(connection, payer, mint, ata.address, payer, INITIAL_MINT_AMOUNT);
      balances[0] = INITIAL_MINT_AMOUNT;
    }

    wallets.push(wallet);
    tokenAccounts.push(ata);
    console.log(`  Wallet ${i}: ${wallet.publicKey.toBase58()}`);
  }

  console.log("\nStarting transfer loop. Ctrl+C to stop.\n");

  let iteration = 0;
  while (true) {
    // Only pick senders with a real, locally-tracked balance — avoids
    // spamming failed transactions from empty accounts. Alternates
    // between Transfer and TransferChecked so both instruction shapes
    // actually get exercised, since the indexer needs to handle both.
    const candidates = balances.map((bal, idx) => ({ bal, idx })).filter((w) => w.bal > 0);
    const from = candidates[Math.floor(Math.random() * candidates.length)];
    let toIndex = Math.floor(Math.random() * NUM_WALLETS);
    while (toIndex === from.idx) toIndex = Math.floor(Math.random() * NUM_WALLETS);

    const amount = Math.max(1, Math.floor(from.bal * (0.01 + Math.random() * 0.49)));
    const useChecked = iteration % 2 === 0;

    const tx = new Transaction();
    tx.add(
      useChecked
        ? createTransferCheckedInstruction(
            tokenAccounts[from.idx].address,
            mint,
            tokenAccounts[toIndex].address,
            wallets[from.idx].publicKey,
            amount,
            MINT_DECIMALS,
          )
        : createTransferInstruction(
            tokenAccounts[from.idx].address,
            tokenAccounts[toIndex].address,
            wallets[from.idx].publicKey,
            amount,
          ),
    );

    try {
      const sig = await sendAndConfirmTransaction(connection, tx, [wallets[from.idx]], {
        commitment: "confirmed",
      });
      balances[from.idx] -= amount;
      balances[toIndex] += amount;
      console.log(
        `[${useChecked ? "TransferChecked" : "Transfer"}] wallet ${from.idx} -> ${toIndex}, amount ${amount}, sig ${sig.slice(0, 12)}...`,
      );
    } catch (err) {
      console.log(`  transfer failed: ${(err as Error).message.slice(0, 100)}`);
    }

    iteration++;
    await new Promise((resolve) => setTimeout(resolve, TRANSFER_INTERVAL_MS));
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});