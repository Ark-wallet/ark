#[macro_use]
extern crate log;

use std::time::Duration;

use bitcoincore_rpc::{bitcoin::amount::Amount, RpcApi};

use ark_testing::{
	daemon::{aspd::AspdHelper, bitcoind::BitcoindHelper}, 
	AspdConfig, 
	Bark, 
	Daemon, 
	TestContext
};

const OFFBOARD_FEES: Amount = Amount::from_sat(900);

async fn setup_simple(name: &str) -> (TestContext, Daemon<BitcoindHelper>, Daemon<AspdHelper>, Bark, Bark) {
	let ctx = TestContext::new(name).await;
	let bitcoind = ctx.bitcoind("bitcoind").await;
	let aspd = ctx.aspd("aspd", &bitcoind, None).await;

	let bark1 = ctx.bark("bark1".to_string(), &bitcoind, &aspd).await;
	let bark2 = ctx.bark("bark2".to_string(), &bitcoind, &aspd).await;

	(ctx, bitcoind, aspd, bark1, bark2)
}

async fn setup_asp_funded(name: &str) -> (TestContext, Daemon<BitcoindHelper>, Daemon<AspdHelper>, Bark, Bark) {
	let (ctx, bitcoind, aspd, bark1, bark2) = setup_simple(name).await;

	// Fund the asp
	bitcoind.prepare_funds().await;
	bitcoind.fund_aspd(&aspd, Amount::from_int_btc(10)).await;

	(ctx, bitcoind, aspd, bark1, bark2)
}

async fn setup_full(name: &str) -> (TestContext, Daemon<BitcoindHelper>, Daemon<AspdHelper>, Bark, Bark) {
	let (ctx, bitcoind, aspd, bark1, bark2) = setup_asp_funded(name).await;

	// Fund clients
	bitcoind.fund_bark(&bark1, Amount::from_sat(1_000_000)).await;
	bitcoind.fund_bark(&bark2, Amount::from_sat(1_000_000)).await;
	bark2.onboard(Amount::from_sat(800_000)).await;

	// refresh vtxo
	bark1.onboard(Amount::from_sat(200_000)).await;
	bark1.refresh_all().await;
	// onboard vtxo
	bark1.onboard(Amount::from_sat(300_000)).await;
	// oor vtxo
	bark2.send_oor(&bark1.vtxo_pubkey().await, Amount::from_sat(330_000)).await;

	(ctx, bitcoind, aspd, bark1, bark2)
}

#[tokio::test]
async fn bark_version() {
	let (_, _, _, bark, _) = setup_simple("bark/bark_version").await;

	let result = bark.run(&[&"--version"]).await;

	assert!(result.starts_with("bark-client"));
}

#[tokio::test]
async fn bark_create_is_atomic() {
	let ctx = TestContext::new("bark/bark_create_is_atomic").await;
	let bitcoind = ctx.bitcoind("bitcoind").await;
	let mut aspd = ctx.aspd("aspd", &bitcoind, None).await;

	// Create a bark defines the folder
	let _  = ctx.try_bark("bark_ok", &bitcoind, &aspd).await.expect("Can create bark");
	assert!(std::path::Path::is_dir(ctx.datadir.join("bark_ok").as_path()));

	// You can't create a bark twice
	// If you want to overwrite the folder you need force
	let _ = ctx.try_bark("bark_twice", &bitcoind, &aspd).await.expect("Can create bark");
	assert!(std::path::Path::is_dir(ctx.datadir.join("bark_twice").as_path()));

	let _ = ctx.try_bark("bark_twice", &bitcoind, &aspd).await.expect_err("Can create bark");
	assert!(std::path::Path::is_dir(ctx.datadir.join("bark_twice").as_path()));

	// We stop the asp
	// This ensures that clients cannot be created
	aspd.stop().await.unwrap();
	let _ = ctx.try_bark("bark_fails", &bitcoind, &aspd).await.expect_err("Cannot create bark if asp is not available");
	assert!(!std::path::Path::is_dir(ctx.datadir.join("bark_fails").as_path()));
}

#[tokio::test]
async fn onboard_bark() {
	const ONBOARD_AMOUNT: u64 = 90_000;
	let (_ctx, bitcoind, _aspd, bark1, _bark2) = setup_simple("bark/onboard_bark").await;

	// Generate initial funds
	bitcoind.prepare_funds().await;

	// Get the bark-address and fund it
	bitcoind.fund_bark(&bark1, Amount::from_sat(100_000)).await;
	bark1.onboard(Amount::from_sat(ONBOARD_AMOUNT)).await;

	assert_eq!(Amount::from_sat(ONBOARD_AMOUNT), bark1.offchain_balance().await);
}

#[tokio::test]
async fn onboard_all_bark() {
	let (_ctx, bitcoind, _aspd, bark1, _bark2) = setup_simple("bark/onboard_all_bark").await;

	// Generate initial funds
	bitcoind.prepare_funds().await;

	// Get the bark-address and fund it
	let funding_txid = bitcoind.fund_bark(&bark1, Amount::from_sat(100_000)).await;
	bark1.onboard_all().await;

	// Check that we emptied our on-chain balance
	assert_eq!(bark1.onchain_balance().await, Amount::ZERO);

	// Check if the onboarding tx's output value is the same as our off-chain balance
	let sync_client = bitcoind.sync_client();
	let entry = sync_client.get_mempool_entry(&funding_txid).unwrap();
	let onboard_txid = entry.spent_by.last().unwrap();
	let onboard_tx = sync_client.get_raw_transaction(onboard_txid, None).unwrap();
	assert_eq!(bark1.offchain_balance().await, onboard_tx.output.last().unwrap().value - ark::onboard::onboard_surplus());
}

#[tokio::test]
async fn list_vtxos() {
	let (_ctx, _bitcoind, mut aspd, bark1, _bark2) = setup_full("bark/list_vtxos").await;

	let vtxos = bark1.vtxos().await;
	assert_eq!(3, vtxos.len());
	assert!(vtxos.iter().any(|v| v.amount.to_sat() == 200_000));
	assert!(vtxos.iter().any(|v| v.amount.to_sat() == 300_000));
	assert!(vtxos.iter().any(|v| v.amount.to_sat() == 330_000));

	// Should have the same behaviour when ASP is offline
	aspd.stop().await.unwrap();

	let vtxos = bark1.vtxos().await;
	assert_eq!(3, vtxos.len());
	assert!(vtxos.iter().any(|v| v.amount.to_sat() == 200_000));
	assert!(vtxos.iter().any(|v| v.amount.to_sat() == 300_000));
	assert!(vtxos.iter().any(|v| v.amount.to_sat() == 330_000));
}


#[tokio::test]
async fn large_round() {
	let ctx = TestContext::new("bark/large_round").await;
	#[cfg(not(feature = "slow_test"))]
	const N: usize = 9;
	#[cfg(feature = "slow_test")]
	const N: usize = 74;

	info!("Running multiple_round_test with N set to {}", N);

	// Initialize the test
	let bitcoind = ctx.bitcoind("bitcoind").await;
	let aspd_cfg = AspdConfig {
		round_interval: Duration::from_millis(2_000),
		round_submit_time: Duration::from_millis(100 * N as u64),
		round_sign_time: Duration::from_millis(1000 * N as u64),
		nb_round_nonces: 200,
		..ctx.aspd_default_cfg("aspd", &bitcoind, None).await
	};
	let aspd = ctx.aspd_with_cfg("aspd", aspd_cfg).await;

	// Fund the asp
	bitcoind.prepare_funds().await;
	bitcoind.fund_aspd(&aspd, Amount::from_int_btc(10)).await;

	// Create a few clients
	let (barks, pks) = {
		let mut barks = Vec::new();
		let mut pks = Vec::new();
		for i in 0..N {
			let b = ctx.bark(format!("bark{}", i), &bitcoind, &aspd).await;
			// Provide onchain funds
			bitcoind.fund_bark(&b, Amount::from_sat(90_000)).await;
			if i % 24 == 0 {
				bitcoind.generate(1).await;
			}
			pks.push(b.vtxo_pubkey().await);
			barks.push(b);
		}
		(barks, pks)
	};

	// Onboard all vtxo's
	for chunk in barks.chunks(20) { // 25 sometimes failed..
		futures::future::join_all(chunk.iter().map(|b| {
			b.onboard(Amount::from_sat(80_000))
		})).await;
		bitcoind.generate(1).await;
	}

	// Refresh all vtxos
	let pks_shifted = pks.iter().chain(pks.iter()).skip(1).cloned().take(N).collect::<Vec<_>>();
	//TODO(stevenroose) need to find a way to ensure that all these happen in the same round
	futures::future::join_all(barks.iter().zip(pks_shifted).map(|(b, _pk)| {
		b.refresh_all()
	})).await;
}

#[tokio::test]
async fn oor() {
	// Initialize the test
	let (_ctx, bitcoind, _aspd, bark1, bark2) = setup_asp_funded("bark/oor").await;

	// Fund clients
	bitcoind.fund_bark(&bark1, Amount::from_sat(90_000)).await;
	bitcoind.fund_bark(&bark2, Amount::from_sat(5_000)).await;
	bark1.onboard(Amount::from_sat(80_000)).await;

	let pk2 = bark2.vtxo_pubkey().await;
	bark1.send_oor(pk2, Amount::from_sat(20_000)).await;

	assert_eq!(58_035, bark1.offchain_balance().await.to_sat());
	assert_eq!(20_000, bark2.offchain_balance().await.to_sat());
}

#[tokio::test]
async fn refresh() {
	let (_ctx, bitcoind, _aspd, bark1, bark2) = setup_asp_funded("bark/refresh").await;

	// Fund clients
	bitcoind.fund_bark(&bark1, Amount::from_sat(1_000_000)).await;
	bitcoind.fund_bark(&bark2, Amount::from_sat(1_000_000)).await;
	bark1.onboard(Amount::from_sat(800_000)).await;
	bark2.onboard(Amount::from_sat(800_000)).await;

	// We want bark2 to have a refresh, onboard, round and oor vtxo
	let pk1 = bark1.vtxo_pubkey().await;
	let pk2 = bark2.vtxo_pubkey().await;
	bark2.send_oor(&pk1, Amount::from_sat(20_000)).await; // generates change
	bark1.refresh_all().await;
	bark1.send_oor(&pk2, Amount::from_sat(20_000)).await;
	bark2.onboard(Amount::from_sat(20_000)).await;

	assert_eq!(3, bark2.vtxos().await.len());
	bark2.refresh_all().await;
	assert_eq!(1, bark2.vtxos().await.len());
}

#[tokio::test]
async fn compute_balance() {
	let (_ctx, _bitcoind, mut aspd, bark1, _bark2) = setup_full("bark/compute_balance").await;

	let balance = bark1.offchain_balance().await;
	assert_eq!(balance, Amount::from_sat(830_000));

	// Should have the same behaviour when ASP is offline
	aspd.stop().await.unwrap();

	let balance = bark1.offchain_balance().await;
	assert_eq!(balance, Amount::from_sat(830_000));
}

#[tokio::test]
async fn offboard_all() {
	let (_ctx, bitcoind, _aspd, bark1, _bark2) = setup_full("bark/offboard_all").await;

	let address = bitcoind.get_new_address();

	let init_balance = bark1.offchain_balance().await;
	assert_eq!(init_balance, Amount::from_sat(830_000));

	bark1.offboard_all(address.clone()).await;

	// We check that all vtxos have been offboarded
	assert_eq!(Amount::ZERO, bark1.offchain_balance().await);

	// We check that provided address received the coins
	bitcoind.generate(1).await;
	let balance = bitcoind.get_received_by_address(&address);
	assert_eq!(balance, init_balance - OFFBOARD_FEES);
}

#[tokio::test]
async fn offboard_vtxos() {
	let (_ctx, bitcoind, _aspd, bark1, _bark2) = setup_full("bark/offboard_vtxos").await;

	let vtxos = bark1.vtxos().await;
	assert_eq!(3, vtxos.len());

	let address = bitcoind.get_new_address();
	let vtxo_to_offboard = &vtxos[1];

	bark1.offboard_vtxo(vtxo_to_offboard.id, address.clone()).await;

	// We check that only selected vtxo has been touched
	let updated_vtxos = bark1.vtxos().await
		.into_iter()
		.map(|vtxo| vtxo.id)
		.collect::<Vec<_>>();

	assert!(updated_vtxos.contains(&vtxos[0].id));
	assert!(updated_vtxos.contains(&vtxos[2].id));

	// We check that provided address received the coins
	bitcoind.generate(1).await;
	let balance = bitcoind.get_received_by_address(&address);
	assert_eq!(balance, vtxo_to_offboard.amount - OFFBOARD_FEES);
}

#[tokio::test]
async fn drop_vtxos() {
	// Initialize the test
	let (_ctx, _bitcoind, _aspd, bark1, _bark2) = setup_full("bark/drop_vtxos").await;

	bark1.drop_vtxos().await;
	let balance = bark1.offchain_balance_no_sync().await;

	assert_eq!(balance, Amount::ZERO);
}
