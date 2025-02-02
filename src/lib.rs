use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::account_info::{next_account_info, AccountInfo};
use solana_program::program_error::ProgramError;
use solana_program::pubkey::Pubkey;
use solana_program::{declare_id, entrypoint, msg, system_program};
use solana_program::entrypoint::ProgramResult;
use std::slice::Iter;

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct Multisig {
    pub owners: Vec<Pubkey>,
    pub threshold: u8,
    pub signers: Vec<bool>,
}

#[derive(BorshSerialize, BorshDeserialize)]
pub enum MultisigInstruction {
    Create { owners: Vec<Pubkey>, threshold: u8 },
    Sign,
    Execute { amount: u64, destination: Pubkey },
}

// program's public key (after generating keypair)
declare_id!("mw45AnZJJU8iUMkRNgytM11J7b4VAi6ptzViWHJ9mbD");
entrypoint!(process_instruction);

pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    // `ID` comes from the `declare_id!` macro. When we use `declare_id!`, it creates a static constant called `ID`.
    if program_id != &ID  {
        msg!("Invalid program ID");
        return Err(ProgramError::IncorrectProgramId);
    }

    let instruction = MultisigInstruction::try_from_slice(instruction_data)?;
    let account_info_iter = &mut accounts.iter();

    match instruction {
        MultisigInstruction::Create { owners, threshold } => {
            process_create(account_info_iter, owners, threshold)
        }
        MultisigInstruction::Sign => process_sign(account_info_iter),
        MultisigInstruction::Execute {
            amount,
            destination,
        } => process_execute(account_info_iter, amount, destination),
    }
}

fn process_create(
    account_info_iter: &mut Iter<AccountInfo>,
    owners: Vec<Pubkey>,
    threshold: u8,
) -> ProgramResult {
    let multisig_account = next_account_info(account_info_iter)?;
    if !multisig_account.is_writable {
        return Err(ProgramError::InvalidAccountData);
    }
    if threshold == 0 || threshold as usize > owners.len() {
        return Err(ProgramError::InvalidArgument);
    }

    // Create the multisig structure
    let multisig = Multisig {
        owners: owners.clone(),
        threshold,
        signers: vec![false; owners.len()],
    };

    // Get a mutable reference to the data
    let mut data = multisig_account.try_borrow_mut_data()?;
    // Clear the existing data
    // data[..].fill(0);

    // Serialize the multisig structure into the account data
    let mut writer = std::io::Cursor::new(&mut data[..]);
    multisig.serialize(&mut writer)?;

    Ok(())
}

fn process_sign(account_info_iter: &mut Iter<AccountInfo>,) -> ProgramResult {
    let signer = next_account_info(account_info_iter)?;
    let multisig_account = next_account_info(account_info_iter)?;

    if !signer.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Debug prints
    msg!("Account data length: {}", multisig_account.data.borrow().len());
    msg!("Account data: {:?}", &multisig_account.data.borrow()[..]);

    // Read the current state
    let mut multisig = Multisig::try_from_slice(&multisig_account.data.borrow())?;
    msg!("Successfully deserialized multisig");

    // Find and update signer
    let signer_index = multisig
        .owners
        .iter()
        .position(|owner| owner == signer.key)
        .ok_or(ProgramError::InvalidArgument)?;

    multisig.signers[signer_index] = true;

    // Get the required space
    let mut temp_buffer = vec![];
    multisig.serialize(&mut temp_buffer)?;
    let required_space = temp_buffer.len();

    msg!("Required space: {}, Available space: {}",
        required_space,
        multisig_account.data.borrow().len()
    );

    // Ensure we have enough space
    if required_space > multisig_account.data.borrow().len() {
        return Err(ProgramError::AccountDataTooSmall);
    }

    // Write the data
    let mut data = multisig_account.try_borrow_mut_data()?;
    // data[..].fill(0);  // Clear existing data
    multisig.serialize(&mut &mut data[..])?;

    Ok(())
}

fn process_execute(
    account_info_iter: &mut Iter<AccountInfo>,
    amount: u64,
    destination: Pubkey,
) -> ProgramResult {
    let multisig_account = next_account_info(account_info_iter)?;
    let destination_account = next_account_info(account_info_iter)?;
    let system_program = next_account_info(account_info_iter)?;

    println!("In process_execute - Account data length: {}", multisig_account.data.borrow().len());
    println!("In process_execute - Account is_writable: {}", multisig_account.is_writable);
    println!("Execute amount: {}, destination: {}", amount, destination);

    // Verify accounts
    if !multisig_account.is_writable {
        return Err(ProgramError::InvalidAccountData);
    }

    if destination_account.key != &destination {
        return Err(ProgramError::InvalidArgument);
    }

    if system_program.key != &system_program::ID {
        return Err(ProgramError::InvalidArgument);
    }

    // Read the current multisig state
    let multisig = Multisig::try_from_slice(&multisig_account.data.borrow())?;
    println!("Current multisig state: {:?}", multisig);

    // Count the number of signatures
    let signature_count = multisig.signers.iter().filter(|&&signed| signed).count();
    println!("Signature count: {}, Required threshold: {}", signature_count, multisig.threshold);

    // Check if we have enough signatures
    if signature_count < multisig.threshold as usize {
        return Err(ProgramError::InsufficientFunds); // Using this error for "insufficient signatures"
    }

    // Check if multisig has enough funds
    if multisig_account.lamports() < amount {
        return Err(ProgramError::InsufficientFunds);
    }

    // Transfer funds
    **multisig_account.try_borrow_mut_lamports()? -= amount;
    **destination_account.try_borrow_mut_lamports()? += amount;

    // Reset the signers after successful execution
    let mut updated_multisig = multisig;
    updated_multisig.signers = vec![false; updated_multisig.owners.len()];

    println!("Updated multisig state after reset: {:?}", updated_multisig);

    // Get a mutable reference to the data
    let mut data = multisig_account.try_borrow_mut_data()?;
    updated_multisig.serialize(&mut &mut data[..])?;

    println!("After serialize - Account data length: {}", data.len());
    Ok(())
}