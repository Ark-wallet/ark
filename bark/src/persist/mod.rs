pub mod sqlite;

use ark::{Vtxo, VtxoId};
use bdk_wallet::WalletPersister;
use bitcoin::{secp256k1::PublicKey, Amount};
use serde::ser::StdError;

use crate::{exit::ExitIndex, Config, Pagination, WalletProperties, vtxo_state::VtxoState, MovementArgs, Movement};

pub trait WalletPersisterError: 'static + std::fmt::Debug + std::fmt::Display + Send + Sync + StdError {}

pub trait BarkPersister: Clone + WalletPersister + Send + Sync {
	/// Initialise wallet in the database
	///
	/// Will fail after first call
	fn init_wallet(&self, config: &Config, properties: &WalletProperties) -> anyhow::Result<()>;

	fn write_config(&self, config: &Config) -> anyhow::Result<()>;
	fn read_properties(&self) -> anyhow::Result<Option<WalletProperties>>;
	fn read_config(&self) -> anyhow::Result<Option<Config>>;

	/// Check if given recipient exists in the database
	fn check_recipient_exists(&self, recipient: &str) -> anyhow::Result<bool>;
	/// Returns a paginated list of movements
	fn get_paginated_movements(&self, pagination: Pagination) -> anyhow::Result<Vec<Movement>>;
	/// Register a movement
	fn register_movement<'a, S, R, Re>(
		&self,
		movement: MovementArgs<'a, S, R, Re>
	) -> anyhow::Result<()>
		where
			S: IntoIterator<Item = &'a Vtxo>,
			R: IntoIterator<Item = (&'a Vtxo, VtxoState)>,
			Re: IntoIterator<Item = (String, Amount)>;

	/// Fetch a VTXO by id in the database
	fn get_vtxo(&self, id: VtxoId) -> anyhow::Result<Option<Vtxo>>;
	/// Fetch all VTXO's that are in a given state
	fn get_vtxos_by_state(&self, state: &[VtxoState]) -> anyhow::Result<Vec<Vtxo>>;
	/// Get the soonest-expiring vtxos to cover provided amount.
	///
	/// Returns an error if the amount cannot be covered by the available
	/// vtxos.
	fn select_vtxos_to_cover(&self, amount: Amount) -> anyhow::Result<Vec<Vtxo>>;
	/// Remove a VTXO from the database
	fn remove_vtxo(&self, id: VtxoId) -> anyhow::Result<Option<Vtxo>>;
	/// Check whether a VTXO has been spent already or not
	fn has_spent_vtxo(&self, id: VtxoId) -> anyhow::Result<bool>;

	/// Store a newly revealed index
	fn store_vtxo_key_index(&self, index: u32, public_key: PublicKey) -> anyhow::Result<()>;
	/// Get last revealed index
	fn get_last_vtxo_key_index(&self) -> anyhow::Result<Option<u32>>;
	/// Get index of vtxo key
	fn get_vtxo_key_index(&self, vtxo: &Vtxo) -> anyhow::Result<u32>;
	/// Checks if provided public key exists in the database,
	/// meaning that it is owned by the wallet
	fn check_vtxo_key_exists(&self, public_key: &PublicKey) -> anyhow::Result<bool>;

	/// Store the ongoing exit process.
	fn store_exit(&self, exit: &ExitIndex) -> anyhow::Result<()>;
	/// Fetch an ongoing exit process.
	fn fetch_exit(&self) -> anyhow::Result<Option<ExitIndex>>;

	fn get_last_ark_sync_height(&self) -> anyhow::Result<u32>;
	fn store_last_ark_sync_height(&self, height: u32) -> anyhow::Result<()>;

	fn update_vtxo_state_checked(&self, vtxo_id: VtxoId, new_state: VtxoState, allowed_old_states: &[VtxoState]) -> anyhow::Result<()>;

	/// Fetch all currently spendable VTXOs in the database
	fn get_all_spendable_vtxos(&self) -> anyhow::Result<Vec<Vtxo>> {
		self.get_vtxos_by_state(&[VtxoState::Spendable])
	}
}
