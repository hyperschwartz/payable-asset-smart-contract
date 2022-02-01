use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Addr, Decimal, Storage, Uint128};
use cosmwasm_storage::{
    bucket, bucket_read, singleton, singleton_read, Bucket, ReadonlyBucket, ReadonlySingleton,
    Singleton,
};

pub static CONFIG_KEY: &[u8] = b"config";
pub static PAYABLE_META_KEY: &[u8] = b"payable_meta";

/// Fields that comprise the smart contract state
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct State {
    // Name of the contract that is tagged on various things
    pub contract_name: String,
    // Cost to onboard each payable
    pub onboarding_cost: Uint128,
    // Coin type for onboarding charge
    pub onboarding_denom: String,
    // The address that will collect onboarding fees
    pub fee_collection_address: Addr,
    // Percentage of each transaction that is taken as fee
    pub fee_percent: Decimal,
    // Address of the oracle application that can withdraw excess fees after fee percent is removed from onboarding_cost
    pub oracle_address: Addr,
}

pub fn config(storage: &mut dyn Storage) -> Singleton<State> {
    singleton(storage, CONFIG_KEY)
}

pub fn config_read(storage: &dyn Storage) -> ReadonlySingleton<State> {
    singleton_read(storage, CONFIG_KEY)
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct PayableMeta {
    // The unique identifier for the payable. Used for all requests that target a payable and the key to the Bucket
    pub payable_uuid: String,
    // The address of the scope created during onboarding of a payable
    pub scope_id: String,
    // The denomination the payable accepts for payment
    pub payable_denom: String,
    // The amount of payable_denom that the payable was originally created to reflect
    pub payable_total_owed: Uint128,
    // The amount of payable_denom left unpaid on the payable
    pub payable_remaining_owed: Uint128,
    // Whether or not the oracle has reviewed the structure of the payable and determine if it is
    // a valid payable
    pub oracle_approved: bool,
}

pub fn payable_meta_storage(storage: &mut dyn Storage) -> Bucket<PayableMeta> {
    bucket(storage, PAYABLE_META_KEY)
}

pub fn payable_meta_storage_read(storage: &dyn Storage) -> ReadonlyBucket<PayableMeta> {
    bucket_read(storage, PAYABLE_META_KEY)
}
