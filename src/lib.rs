#![warn(clippy::all)]
pub mod contract;
pub mod error;
pub mod helper;
pub mod msg;
pub mod register_payable;
pub mod state;

#[cfg(target_arch = "wasm32")]
cosmwasm_std::create_entry_points_with_migration!(contract);
