use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Addr, Decimal, Storage, Uint128};
use cosmwasm_storage::{
    bucket, bucket_read, singleton, singleton_read, Bucket, ReadonlyBucket, ReadonlySingleton,
    Singleton,
};

// TODO: Remove these two keys after the contract has been migrated to StateV2 and PayableMetaV2
pub static CONFIG_KEY: &[u8] = b"config";
pub static PAYABLE_META_KEY: &[u8] = b"payable_meta";

pub static CONFIG_KEY_V2: &[u8] = b"config_v2";
pub static PAYABLE_META_V2_KEY: &[u8] = b"payable_meta_v2";

// TODO: Remove after the contract is fully migrated to StateV2
/// Fields that comprise the smart contract state
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct State {
    // The type of payable that this contract handles
    pub payable_type: String,
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
    // Whether nor not the contract is running locally.  Skips some important checks if enabled, which expedites testing
    pub is_local: bool,
}

// TODO: Remove when all code uses config_v2 only
pub fn config(storage: &mut dyn Storage) -> Singleton<State> {
    singleton(storage, CONFIG_KEY)
}

// TODO: Remove when all code uses config_read_v2 only
pub fn config_read(storage: &dyn Storage) -> ReadonlySingleton<State> {
    singleton_read(storage, CONFIG_KEY)
}

/// Stores all relevant data about the contract. Modifiable only partially by migrations
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct StateV2 {
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
    // Whether nor not the contract is running locally.  Skips some important checks if enabled, which expedites testing
    pub is_local: bool,
}

pub fn config_v2(storage: &mut dyn Storage) -> Singleton<StateV2> {
    singleton(storage, CONFIG_KEY_V2)
}

pub fn config_read_v2(storage: &dyn Storage) -> ReadonlySingleton<StateV2> {
    singleton_read(storage, CONFIG_KEY_V2)
}

// TODO: Remove after the live contract is fully migrated to PayableMetaV2
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

/// This struct is serialized directly as an attribute on each payable's scope
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct PayableScopeAttribute {
    // The name of the asset classification for this payable
    pub payable_type: String,
    // The unique identifier for the payable. Used for all requests that target a payable and the key to the Bucket
    pub payable_uuid: String,
    // The address of the scope created during onboarding of a payable
    pub scope_id: String,
    // The address of the oracle that handles validation for this payable
    pub oracle_address: Addr,
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

/// This struct is used to link a payable uuid to a scope id to allow querying for PayableScopeAttribute
/// data when a scope id is not available to the caller
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct PayableMetaV2 {
    // The unique identifier for the payable. Used for all requests that target a payable and the key to the Bucket
    pub payable_uuid: String,
    // The address of the scope created during onboarding of a payable
    pub scope_id: String,
}

// TODO: Remove when all code uses payable_meta_storage_v2 only
pub fn payable_meta_storage(storage: &mut dyn Storage) -> Bucket<PayableMeta> {
    bucket(storage, PAYABLE_META_KEY)
}

// TODO: Remove when all code uses payable_meta_storage_read_v2 only
pub fn payable_meta_storage_read(storage: &dyn Storage) -> ReadonlyBucket<PayableMeta> {
    bucket_read(storage, PAYABLE_META_KEY)
}

pub fn payable_meta_storage_v2(storage: &mut dyn Storage) -> Bucket<PayableMetaV2> {
    bucket(storage, PAYABLE_META_V2_KEY)
}

pub fn payable_meta_storage_read_v2(storage: &dyn Storage) -> ReadonlyBucket<PayableMetaV2> {
    bucket_read(storage, PAYABLE_META_V2_KEY)
}
