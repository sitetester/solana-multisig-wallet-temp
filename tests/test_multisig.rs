use borsh::{BorshDeserialize, BorshSerialize};
use solana_multisig_wallet::{process_instruction, Multisig, MultisigInstruction};
use solana_program::instruction::AccountMeta;
use solana_program::pubkey::Pubkey;
use solana_program::system_program;
use solana_program_test::{processor, ProgramTest, ProgramTestContext};
use solana_sdk::account::Account;
use solana_sdk::signature::Keypair;
use solana_sdk::signer::Signer;
use solana_sdk::transaction::Transaction;
use std::str::FromStr;

// Calculates space (in bytes)
fn calculate_space(multisig: &Multisig) -> usize {
    let mut space_buffer = vec![];
    // `serialize` => convert strut to bytes
    multisig.serialize(&mut space_buffer).unwrap();
    // find/calculate space for storing `multisig` bytes on Solana (as it can only store bytes)
    // this is needed for allocating the correct amount of storage since we pay for it
    space_buffer.len()
}

fn debug_print(text: &str) {
    println!("=== {text} ===");
}

#[tokio::test]
async fn test_complete_multisig_flow() {
    debug_print("Starting test_complete_multisig_flow");
    debug_print("1. CREATE TRANSACTION");

    // this one is actually used in code
    let program_id = Pubkey::from_str("mw45AnZJJU8iUMkRNgytM11J7b4VAi6ptzViWHJ9mbD").unwrap();
    // initialize program test environment
    let program_test = ProgramTest::new(
        "solana-multisig-wallet",
        program_id,
        processor!(process_instruction),
    );

    let mut context: ProgramTestContext = program_test.start_with_context().await;

    let owner1_keypair = Keypair::new();
    let owner2_keypair = Keypair::new();
    let owner3_keypair = Keypair::new();

    let owners = vec![
        owner1_keypair.pubkey(),
        owner2_keypair.pubkey(),
        owner3_keypair.pubkey(),
    ];
    // number of signatures required to execute transaction
    let num_signatures = 2u8;

    let multisig = Multisig {
        owners: owners.clone(), // owners' public keys
        threshold: num_signatures,
        // initialize all signatures as false (a fresh multisig transaction where no owner has signed yet)
        signers: vec![false; owners.len()],
    };

    let space = calculate_space(&multisig);

    // `rent` holds Solana's rent formula
    let rent = context.banks_client.get_rent().await.unwrap();
    // find how much to pay for rent based on our `multisig` data size
    let rent_cost = rent.minimum_balance(space);

    // the amount (in lamports) we want to transfer later (in these tests to another account)
    let transfer_amount = 50;
    // total amount (like a prepaid card, activation + spending). Need both.
    let lamports = rent_cost + transfer_amount;

    let multisig_keypair = Keypair::new();
    let multisig_key = multisig_keypair.pubkey();
    // this creates ONLY the basic account structure
    let create_account_instr = solana_sdk::system_instruction::create_account(
        &context.payer.pubkey(), // test wallet payer
        &multisig_key,           // new account address
        lamports,                // initial deposit amount
        space as u64,            // storage space needed
        &program_id,             // program which owns this account
    );

    // Create multisig instruction
    let multisig_create_instr = MultisigInstruction::Create {
        owners: owners.clone(),    // who can sign
        threshold: num_signatures, // how many can sign
    };

    let create_instruction_bytes = multisig_create_instr.try_to_vec().unwrap();

    // create a new multisig (group) account by combining three essential pieces
    let create_multisig_instr = solana_sdk::instruction::Instruction::new_with_bytes(
        program_id,                // which program to run / will handle instructions
        &create_instruction_bytes, // group account details
        vec![
            // accounts to work with
            AccountMeta::new(multisig_key, false), // new account that will be created, but can also be written to, check `new(...)`
            AccountMeta::new_readonly(system_program::id(), false), // system program
        ],
    );

    // Create and send transaction
    let recent_blockhash = context.last_blockhash;
    let transaction = Transaction::new_signed_with_payer(
        // instructions to execute in order
        // first is like setting up an empty account & the second sets up multisig features
        &[create_account_instr, create_multisig_instr],
        Some(&context.payer.pubkey()), // test wallet paying for fees
        &[&context.payer, &multisig_keypair], // required signatures
        recent_blockhash,
    );
    context
        .banks_client
        .process_transaction(transaction)
        .await
        .unwrap();

    // Verify multisig account was created correctly
    let multisig_account = ctx_get_account(&mut context, multisig_key).await;
    let stored_multisig = Multisig::try_from_slice(&multisig_account.data).unwrap();
    assert_eq!(stored_multisig.owners, owners, "Owners don't match");
    assert_eq!(
        stored_multisig.threshold, num_signatures,
        "Threshold doesn't match"
    );
    assert_eq!(
        stored_multisig.signers,
        vec![false; owners.len()],
        "Signers not properly initialized"
    );
    debug_print("1. CREATE TRANSACTION - DONE");

    // ---------------------------------------------------------------------
    // create END
    // ---------------------------------------------------------------------
    debug_print("2. SIGN TRANSACTION");

    // 2. SIGN TRANSACTION
    let sign_instr_bytes = MultisigInstruction::Sign.try_to_vec().unwrap();

    // Create sign instruction. All params are passed to `process_instruction()`
    let sign_instr = solana_sdk::instruction::Instruction::new_with_bytes(
        program_id,        // program that will process this signing instruction
        &sign_instr_bytes, // serialized "Sign" enum variant
        vec![
            // these accounts are
            // person signing the tx, must be in owners list of multisig_key AND must sign the tx
            AccountMeta::new_readonly(owner1_keypair.pubkey(), true),
            // multisig account being signed, doesn't sign, but is writeable inside new()
            AccountMeta::new(multisig_key, false),
        ],
    );

    // Print initial state (useful for debugging)
    let initial_account = ctx_get_account(&mut context, multisig_key).await;
    let initial_multisig = Multisig::try_from_slice(&initial_account.data).unwrap();
    println!("\n=== Before Signing ===");
    println!("Initial multisig state: {:?}", initial_multisig);

    // Create and send sign transaction
    let recent_blockhash = context.last_blockhash;
    let sign_transaction = Transaction::new_signed_with_payer(
        &[sign_instr],
        Some(&context.payer.pubkey()),
        &[&context.payer, &owner1_keypair], // Both payer and owner1 need to sign
        recent_blockhash,
    );

    // Process the sign transaction
    context
        .banks_client
        .process_transaction(sign_transaction)
        .await
        .unwrap();

    // Verify the state after signing
    let multisig_account = ctx_get_account(&mut context, multisig_key).await;
    let stored_multisig = Multisig::try_from_slice(&multisig_account.data).unwrap();

    // Verify the signing state
    println!("\n=== After Signing ===");
    println!("Stored multisig: {:?}", stored_multisig);

    assert_eq!(
        stored_multisig.owners, owners,
        "Owners don't match after signing"
    );
    assert_eq!(
        stored_multisig.threshold, num_signatures,
        "Threshold doesn't match after signing"
    );
    assert!(stored_multisig.signers[0], "First signer should be true"); // as we already invoked `Sign` instruction above
    assert!(!stored_multisig.signers[1], "Second signer should be false");

    // Optional: Verify account data length hasn't changed
    assert_eq!(
        multisig_account.data.len(),
        space,
        "Account data length changed unexpectedly"
    );
    debug_print("2. SIGN TRANSACTION - DONE");

    // ---------------------------------------------------------------------
    // sign END
    // ---------------------------------------------------------------------
    debug_print("3. EXECUTE TRANSACTION");

    // first we create the destination account
    let destination_keypair = Keypair::new();
    let recipient_key = destination_keypair.pubkey();

    // minimum_balance(0) is the minimum possible rent cost (there is no data storage), it just holds SOL
    let destination_minimum_rent = rent.minimum_balance(0);

    // let's create a new account to receive SOL from the multisig execution
    let create_destination_account_instr = solana_sdk::system_instruction::create_account(
        &context.payer.pubkey(),
        &recipient_key,           // new account address
        destination_minimum_rent, // lamports to ensure rent-exempt
        0,                        // space (0 because just holding SOL, no program data needed)
        &system_program::id(),    // owner (system program owns SOL accounts)
    );

    // create and send transaction to create destination account
    let create_dest_tx = Transaction::new_signed_with_payer(
        &[create_destination_account_instr],
        Some(&context.payer.pubkey()),
        &[
            // payer must sign because it's paying for fees (tx fee & initial rent-exempt balance)
            // system program needs proof that the payer authorized this payment
            &context.payer,
            // new account must sign for its creation, signature proves you have the private key for this new account
            &destination_keypair,
        ],
        // prevents replay attacks, also block hashes are only valid for a limited time window ~2 minutes (150 blocks)
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(create_dest_tx)
        .await
        .unwrap();

    // verify destination account was created
    let dest_account = ctx_get_account(&mut context, recipient_key).await;
    assert_eq!(
        dest_account.lamports, destination_minimum_rent,
        "Destination account should be rent-exempt"
    );

    // ----------------------------------------------------------------
    // destination account created, next testing multisig transfer flow
    // ----------------------------------------------------------------

    // create execute instruction (`Execute` variant of MultisigInstruction enum)
    let multisig_instr_execute = MultisigInstruction::Execute {
        amount: transfer_amount,
        destination: recipient_key,
    };
    let execute_instruction_data = multisig_instr_execute.try_to_vec().unwrap();

    // represents the instruction to execute the multisig transfer
    // first execute attempt (should fail due to insufficient signatures)
    println!("\n=== Calling `Execute` instruction (should fail as multisig_key owner didn't sign tx) ===");

    let multisig_execute_instr = solana_sdk::instruction::Instruction::new_with_bytes(
        program_id,
        &execute_instruction_data,
        vec![
            // `is_signer = false` means this account must be signed at transaction level (but later only payer signs tx)
            AccountMeta::new(multisig_key, false), // will fail, owner didn't sign
            AccountMeta::new(recipient_key, false), // signature not needed
            // system program never signs,
            // needed for native SOL transfers
            AccountMeta::new_readonly(system_program::id(), false),
        ],
    );
    let multisig_execute_tx = Transaction::new_signed_with_payer(
        &[multisig_execute_instr],
        Some(&context.payer.pubkey()),
        &[&context.payer], // only payer signs here, hence it will fail
        context.last_blockhash,
    );
    let result = context.banks_client.process_transaction(multisig_execute_tx).await; // submit tx
    assert!(
        result.is_err(),
        "`Execute` should fail with insufficient signatures as multisig_key has not yet signed"
    );

    // Second owner signs
    println!("\n=== Second Owner Signing ===");
    let sign_ix_2 = solana_sdk::instruction::Instruction::new_with_bytes(
        program_id,
        &sign_instr_bytes,
        vec![ // order matters (because of `process_sign()` logic)
            AccountMeta::new_readonly(owner2_keypair.pubkey(), true),
            AccountMeta::new(multisig_key, false),
        ],
    );

    // Get fresh blockhash
    let recent_blockhash = context.banks_client.get_latest_blockhash().await.unwrap();

    let sign_transaction_2 = Transaction::new_signed_with_payer(
        &[sign_ix_2],
        Some(&context.payer.pubkey()),
        &[&context.payer, &owner2_keypair],
        recent_blockhash,
    );

    context
        .banks_client
        .process_transaction(sign_transaction_2)
        .await
        .unwrap();

    // Record initial balances
    let initial_multisig_balance = ctx_get_account(&mut context, multisig_key).await.lamports;
    let initial_destination_balance = ctx_get_account(&mut context, recipient_key).await.lamports;

    // Get fresh blockhash for final execute
    let recent_blockhash = context.banks_client.get_latest_blockhash().await.unwrap();

    // Final execute attempt (should succeed)
    println!("\n=== Attempting Execute (Should Succeed) ===");
    let execute_ix_2 = solana_sdk::instruction::Instruction::new_with_bytes(
        program_id,
        &execute_instruction_data,
        vec![
            AccountMeta::new(multisig_key, false),
            AccountMeta::new(recipient_key, false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
    );

    let execute_tx_2 = Transaction::new_signed_with_payer(
        &[execute_ix_2],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        recent_blockhash,
    );

    context
        .banks_client
        .process_transaction(execute_tx_2)
        .await
        .unwrap();

    // Verify final state
    let final_multisig_account = ctx_get_account(&mut context, multisig_key).await;
    let final_destination_account = ctx_get_account(&mut context, recipient_key).await;

    // Verify balances
    assert_eq!(
        final_destination_account.lamports,
        initial_destination_balance + transfer_amount,
        "Destination balance incorrect"
    );

    assert_eq!(
        final_multisig_account.lamports,
        initial_multisig_balance - transfer_amount,
        "Multisig balance incorrect"
    );

    // Verify signatures were reset
    let final_multisig = Multisig::try_from_slice(&final_multisig_account.data).unwrap();
    assert!(
        final_multisig.signers.iter().all(|&signed| !signed),
        "Signatures should be reset after execution"
    );

    debug_print("3. EXECUTE TRANSACTION - DONE");
}

async fn ctx_get_account(context: &mut ProgramTestContext, address: Pubkey) -> Account {
    context
        .banks_client
        .get_account(address)
        .await
        .unwrap()
        .unwrap()
}
