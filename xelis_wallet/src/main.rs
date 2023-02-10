pub mod transaction_builder;
pub mod storage;
pub mod wallet;
pub mod config;
pub mod cipher;
pub mod api;

use std::{sync::Arc, time::Duration, path::Path};

use anyhow::{Result, Context};
use config::DIR_PATH;
use fern::colors::Color;
use log::{error, info};
use clap::Parser;
use xelis_common::{config::{
    DEFAULT_DAEMON_ADDRESS,
    VERSION, XELIS_ASSET
}, prompt::{Prompt, command::{CommandManager, Command, CommandHandler, CommandError}, argument::{Arg, ArgType, ArgumentManager}}, async_handler, crypto::{address::{Address, AddressType}, hash::Hashable}, transaction::TransactionType};
use wallet::Wallet;


#[derive(Parser)]
#[clap(version = VERSION, about = "XELIS Wallet")]
pub struct Config {
    /// Daemon address to use
    #[clap(short = 'a', long, default_value_t = String::from(DEFAULT_DAEMON_ADDRESS))]
    daemon_address: String,
    #[clap(short, long, default_value_t = false)]
    offline_mode: bool,
    /// Enable the debug mode
    #[clap(short, long)]
    debug: bool,
    /// Disable the log file
    #[clap(short = 'f', long)]
    disable_file_logging: bool,
    /// Log filename
    #[clap(short = 'l', long, default_value_t = String::from("xelis-wallet.log"))]
    filename_log: String,
    /// Set name path for wallet storage
    #[clap(short, long)]
    name: String,
    /// Password used to open wallet
    #[clap(short, long)]
    password: String
}

#[tokio::main]
async fn main() -> Result<()> {
    let config: Config = Config::parse();
    let prompt = Prompt::new(config.debug, config.filename_log, config.disable_file_logging)?;
    let dir = format!("{}{}", DIR_PATH, config.name);

    let mut wallet = if Path::new(&dir).is_dir() {
        info!("Opening wallet {}", dir);
        Wallet::open(dir, config.password)?
    } else {
        info!("Creating a new wallet at {}", dir);
        Wallet::new(dir, config.password)?
    };

    if !config.offline_mode {
        if let Err(e) = wallet.set_online_mode(&config.daemon_address).await {
            error!("Error while setting online mode: {}", e);
        } else {
            info!("Online mode enabled");
            if let Err(e) = wallet.start_syncing().await {
                error!("Error while syncing: {}", e);
            };
        }
    }

    if let Err(e) = run_prompt(prompt, wallet).await {
        error!("Error while running prompt: {}", e);
    }

    Ok(())
}

async fn run_prompt(prompt: Arc<Prompt>, wallet: Wallet) -> Result<()> {
    let mut command_manager: CommandManager<Wallet> = CommandManager::default();
    command_manager.add_command(Command::with_required_arguments("set_password", "Set a new password to open your wallet", vec![Arg::new("old_password", ArgType::String), Arg::new("password", ArgType::String)], None, CommandHandler::Async(async_handler!(set_password))));
    command_manager.add_command(Command::with_required_arguments("transfer", "Send asset to a specified address", vec![Arg::new("address", ArgType::String), Arg::new("amount", ArgType::Number)], Some(Arg::new("asset", ArgType::String)), CommandHandler::Async(async_handler!(transfer))));
    command_manager.add_command(Command::new("display_address", "Show your wallet address", None, CommandHandler::Async(async_handler!(display_address))));
    command_manager.add_command(Command::new("balance", "Show your current balance", Some(Arg::new("asset", ArgType::String)), CommandHandler::Async(async_handler!(balance))));

    command_manager.set_data(Some(wallet));

    let closure = || async {
        let height_str = format!("{}/{}", 0, 0); // TODO
        let status = Prompt::colorize_str(Color::Red, "Offline");
        format!(
            "{} | {} | {} | {} ",
            Prompt::colorize_str(Color::Blue, "XELIS Wallet"),
            height_str,
            status,
            Prompt::colorize_str(Color::BrightBlack, ">>")
        )
    };
    prompt.start(Duration::from_millis(100), &closure, command_manager).await?;
    Ok(())
}

// Change wallet password
async fn set_password(manager: &CommandManager<Wallet>, mut arguments: ArgumentManager) -> Result<(), CommandError> {
    let wallet = manager.get_data()?;
    let old_password = arguments.get_value("old_password")?.to_string_value()?;
    let password = arguments.get_value("password")?.to_string_value()?;

    manager.message("Changing password...");
    wallet.set_password(old_password, password)?;
    manager.message("Your password has been changed!");
    Ok(())
}

// Create a new transfer to a specified address
async fn transfer(manager: &CommandManager<Wallet>, mut arguments: ArgumentManager) -> Result<(), CommandError> {
    let str_address = arguments.get_value("address")?.to_string_value()?;
    let amount = arguments.get_value("amount")?.to_number()?;
    let address = Address::from_string(&str_address).context("Invalid address")?;

    let asset = if arguments.has_argument("asset") {
        arguments.get_value("asset")?.to_hash()?
    } else {
        XELIS_ASSET // default asset selected is XELIS
    };

    let wallet = manager.get_data()?;
    manager.message("Building transaction...");
    let (key, address_type) = address.split();
    let extra_data = match address_type {
        AddressType::Normal => None,
        AddressType::Data(data) => Some(data)
    };

    let transfer = wallet.create_transfer(asset, key, extra_data, amount)?;
    let tx = wallet.create_transaction(TransactionType::Transfer(vec![transfer]))?;
    let tx_hash = tx.hash();
    manager.message(format!("Transaction hash: {}", tx_hash));

    // TODO send transaction

    Ok(())
}

// Show current wallet address
async fn display_address(manager: &CommandManager<Wallet>, _: ArgumentManager) -> Result<(), CommandError> {
    let wallet = manager.get_data()?;
    manager.message(format!("Wallet address: {}", wallet.get_address()));
    Ok(())
}

// Show current balance for specified asset
async fn balance(manager: &CommandManager<Wallet>, mut arguments: ArgumentManager) -> Result<(), CommandError> {
    let asset = if arguments.has_argument("asset") {
        arguments.get_value("asset")?.to_hash()?
    } else {
        XELIS_ASSET // default asset selected is XELIS
    };

    let wallet = manager.get_data()?;
    let balance = wallet.get_balance(&asset);
    manager.message(format!("Balance for asset {}: {}", asset, balance));

    Ok(())
}