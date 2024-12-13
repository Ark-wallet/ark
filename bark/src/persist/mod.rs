pub mod sqlite;

use ark::{Vtxo, VtxoId};
use bdk_wallet::WalletPersister;
use bitcoin::Amount;

use crate::{exit::Exit, Config, WalletProperties};

pub trait BarkPersister: Clone + WalletPersister {
	/// Initialise wallet in the database
	/// 
	/// Will fail after first call
	fn init_wallet(&self, config: &Config, properties: &WalletProperties) -> anyhow::Result<()>;

	fn write_config(&self, config: &Config) -> anyhow::Result<()>;
	fn read_properties(&self) -> anyhow::Result<Option<WalletProperties>>;
	fn read_config(&self) -> anyhow::Result<Option<Config>>;

	/// Stores a vtxo in the database
	fn store_vtxo(&self, vtxo: &Vtxo) -> anyhow::Result<()>;
	/// Fetch a VTXO by id in the database
	fn get_vtxo(&self, id: VtxoId) -> anyhow::Result<Option<Vtxo>>;
	/// Fetch all currently spendable VTXOs in the database
	fn get_all_spendable_vtxos(&self) -> anyhow::Result<Vec<Vtxo>>;
	/// Get the soonest-expiring vtxos with total value at least `min_value`.
	fn get_expiring_vtxos(&self, min_value: Amount) -> anyhow::Result<Vec<Vtxo>>;
	/// Remove a VTXO from the database
	fn remove_vtxo(&self, id: VtxoId) -> anyhow::Result<Option<Vtxo>>;
	/// Create a new `SPENT` state for the VTXO, making it unspendable again
	fn mark_vtxo_as_spent(&self, id: VtxoId) -> anyhow::Result<()>;
	/// Check whether a VTXO has been spent already or not
	fn has_spent_vtxo(&self, id: VtxoId) -> anyhow::Result<bool>;

	/// Store the ongoing exit process.
	fn store_exit(&self, exit: &Exit) -> anyhow::Result<()>;
	/// Fetch an ongoing exit process.
	fn fetch_exit(&self) -> anyhow::Result<Option<Exit>>;

	fn get_last_ark_sync_height(&self) -> anyhow::Result<u32>;
	fn store_last_ark_sync_height(&self, height: u32) -> anyhow::Result<()>;
}